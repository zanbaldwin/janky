# Reading errors and warnings

When janky finds a problem, it prints a **diagnostic**. A diagnostic is an error or a warning. It
names the problem, the file and the position in that file. This page explains how to read one. The
[Diagnostic codes](../reference/codes.md) page lists every code.

## Errors and warnings

- An **error** stops the command. `plan` shows no plan, `build` writes no package, and the exit
  code is 1.
- A **warning** tells you about something that can be wrong, but the command continues. The exit
  code stays 0.

## Two forms of the same output

In a terminal, janky shows diagnostics in colour, with Unicode marks and frames. If you send the
output to a file or to another program, janky uses plain characters. Most pages of this guide show
the plain form.

| Part | In a terminal | In a file or pipe |
|------|---------------|-------------------|
| Error mark | `×` | `x` |
| Warning mark | `⚠` | `!` |
| Start of the snippet | `╭─[` | `,-[` |
| Underline and label | `───┬──` | `^^^\|^^^` |
| End of the snippet | `╰──` | `` `---- `` |

The `NO_COLOR=1` environment variable removes the colour. It does not change the marks.

## The terminal form

In this example, the entry `pan` in `content/vocab/02-food.yaml` has the key `englsh` in place of
`english`. `janky check` gives two errors:

```console
$ janky check
content::required

  x missing key `english` in `entries[0]`
   ,-[content/vocab/02-food.yaml:5:5]
 4 | entries:
 5 |   - id: 'pan'
   :     ^^^^|^^^^
   :         `-- `english` is missing
 6 |     spanish: 'pan'
   `----
content::unknown-key

  x unknown key `entries[0].englsh`
   ,-[content/vocab/02-food.yaml:7:5]
 6 |     spanish: 'pan'
 7 |     englsh: 'bread'
   :     ^^^|^^
   :        `-- not allowed here
 8 |     gender: 'm'
   `----
...
```

Each diagnostic has these parts:

1. **The code**, on the first line, for example `content::required`. The part before `::` tells you
   the area: `content` for content files, `config` for `deck.toml` and note types, `template` for
   field templates, and so on.
2. **The message**, after `x` or `!`. It says what is wrong.
3. **The position**, after `,-[`. It has the form `file:line:column`. The file path is relative to
   the project directory. Lines and columns start at 1. The column counts characters, not bytes, so
   accented letters such as `ñ` count as one.
4. **The snippet**: the lines around the problem, with line numbers. The `^^^` marks show the exact
   part, and the label below them says more.
5. **The help line**, if janky has advice. It starts with `help:`.

The error names `entries[0]`. That is the first entry in the `entries` list of the file, because
janky counts from 0 in these paths. Use the line number to find it.

Some diagnostics have no file, for example the warning about missing generator results. Then there
is no position and no snippet:

```text
enrich::missing

  ! 6 enrichment results are missing
  help: run `janky generate`
```

### Infos

An info is a note that needs no action, for example a word that the dictionary does not have.
It never stops a command. Without `--verbose`, janky shows all infos of one code as one line:

```text
info: 6 × enrich::info (run with --verbose to see them)
```

With `--verbose`, each info shows in full, in the same form as a warning. `--json` always writes
each info, with `"severity":"info"`.

### A warning with a help line

This warning comes from a data generator that did not find a word. The snippet points at the field
template that asked for the data. The help line names the entry:

```text
enrich::warning

  ! frequency: aguacate: not in the frequency list
    ,-[notetypes/word/notetype.toml:45:9]
 44 | name = "Frequency"
 45 | value = "{% set f = enrich(entry.spanish | nfc, 'frequency') %}{% if f.rank %}#{{ f.rank }} ({{ f.band }}){% endif %}"
    :         ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^|^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    :                                                                `-- `enrich` is called here
 46 |
    `----
  help: first seen in entry `aguacate` in `content/vocab/02-food.yaml`
```

janky keeps this result in its cache, so this warning appears again on each command until you
change the entry or the data. [Generating media and data](../plugins/generating.md) explains how.

### Safe output

janky never prints the value of a secret. It prints `<secret:name>` in its place. Control
characters and characters that change the text direction appear as escape codes of the form
`\u{...}`, so that they cannot hide text. In a snippet, they appear as `?`.

## What janky does with an invalid entry

`janky check` reports all the errors in all files in one run. It does not stop at the first error.

- If a file has a syntax error, or the wrong shape (for example, no `entries` list), janky ignores
  the whole file. The other files still load.
- If an error is in the `defaults` of a file, janky ignores every entry of that file, and reports
  the error once.
- If an error is inside one entry, janky ignores that entry. The other entries of the file are still
  rendered, so `check` also shows their template errors in the same run.

An ignored entry is not "skipped while the rest builds". While there is any error, `plan` and
`build` refuse to run, so nothing builds. Correct every error, then run the command again.

## The JSON form

With `--json`, janky prints each diagnostic as one line of JSON on standard output. JSON is a text
format for data that programs can read. This form is useful for scripts and editor tools. This is
the same typo as above:

```console
$ janky check --json
{"file":"content/vocab/02-food.yaml","line":5,"column":5,"end_line":5,"end_column":14,"severity":"error","code":"content::required","message":"missing key `english` in `entries[0]`","help":null,"related":[]}
{"file":"content/vocab/02-food.yaml","line":7,"column":5,"end_line":7,"end_column":11,"severity":"error","code":"content::unknown-key","message":"unknown key `entries[0].englsh`","help":null,"related":[]}
{"file":"notetypes/word/notetype.toml","line":45,"column":9,"end_line":45,"end_column":119,"severity":"warning","code":"enrich::warning","message":"frequency: aguacate: not in the frequency list","help":"first seen in entry `aguacate` in `content/vocab/02-food.yaml`","related":[]}
```

Each line has these keys:

| Key | Type | Meaning |
|-----|------|---------|
| `file` | text or `null` | The file, relative to the project directory. `null` when the diagnostic has no file. |
| `line` | number or `null` | The line where the problem starts. Counts from 1. |
| `column` | number or `null` | The column where the problem starts, in characters. Counts from 1. |
| `end_line` | number or `null` | The line where the problem ends. |
| `end_column` | number or `null` | The column of the character after the end of the problem. |
| `severity` | text | `"error"`, `"warning"` or `"info"`. |
| `code` | text | The code, for example `"content::required"`. |
| `message` | text | The message. |
| `help` | text or `null` | The advice, or `null` when there is none. |
| `related` | list | More diagnostics in the same form, which give context. For example, a plugin error can have a related diagnostic "the plugin was called here". |

janky prints the diagnostics in the order in which it checks the project. Inside one step, they are
sorted by file and position. Secrets are hidden in the same way as in the terminal form. Control
characters appear as JSON escape codes of the form `\u0000`.

With `--json`, `plan` and `build` also print one plan line at the end.
[Command reference](commands.md#--json-and-the-plan-line) explains it.
