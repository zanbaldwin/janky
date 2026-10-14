# Adding note types

Each `[notetypes.<key>]` table adds one note type to the project. The note type itself is in a
`notetype.toml` file. [Creating note types](../notetypes/index.md) explains that file. This page
explains how `deck.toml` finds it.

A note type can come from your project directory (a **local** note type), or from a Git repository
or an archive on the web (a **remote** note type).

```toml
[notetypes.word]
path = "notetypes/word"
default = true

[notetypes.phrase]
path = "notetypes/phrase.toml"
alias = "expression"
```

## The key is an entry type

The key of the table (`word`, `phrase`) is the identity of the note type in the project. It is
also an **entry type**: an entry with `type: word` uses the `word` note type. Keys follow the
[key rule](decks.md#names-and-keys). Do not change a key after you import the deck.

janky finds the note type of an entry in this order:

1. The `type` of the entry.
2. The `type` in the `defaults` of the content file.
3. The note type with `default = true`.

[Entries: ids, types, tags and defaults](../content/entries.md) explains `type` and `defaults`.

## Local note types

For a local note type, `path` is required. It can point to a file or to a directory:

- A file path, such as `notetypes/phrase.toml`, uses that file.
- A directory path, such as `notetypes/word`, uses `notetype.toml` in that directory.

If the file does not exist, janky gives the error `config::missing-notetype`:

```text
config::missing-notetype
  x `notetypes/missing.toml` does not exist
    `-- no note type file here
```

The paths inside a note type file are relative to the directory that holds that file. They must
stay inside that directory. Several note type files can share one directory.

## Other names: alias

`alias` gives a note type more entry types. It is a string, or a list of strings:

```toml
[notetypes.phrase]
path = "notetypes/phrase.toml"
alias = "expression"
```

```toml
alias = ["expression", "saying"]
```

With this, an entry with `type: expression` also uses the `phrase` note type. The key is always an
entry type, so you do not need to add it to `alias`.

An entry type can belong to one note type only. This includes keys. If an alias of one note type is
the key or an alias of another note type, janky gives the error `config::duplicate-type`:

```text
config::duplicate-type
  x entry type `word` already belongs to note type `vocab`
```

## The default note type

`default = true` makes the note type the default. janky uses it for entries that have no `type`
and no `type` in `defaults`. Only one note type can be the default. A second one gives the error
`config::duplicate-default`:

```text
config::duplicate-default
  x note type `vocab` is already the default
```

## Remote note types

A remote note type comes from a public Git repository or from an archive file. Use one to share a
note type between projects, or to use a note type that someone else made.

> [!NOTE]
> janky puts a copy of the remote note type into each package. Each project gets its own Anki ids,
> so two projects that use the same remote note type make two separate note types in Anki.

### From a Git repository

Git is a tool that stores the history of a set of files. A Git repository can have **tags** (fixed
names for one version), **branches** (lines of work that move forward) and **commits** (single
versions, each with an id).

```toml
[notetypes.grammar]
git = "https://example.com/anki-notetypes.git"
tag = "v1.0"
path = "grammar"
```

| Key | Meaning |
|-----|---------|
| `git` | The URL of a public Git repository. It must start with `https://` or `git://`. |
| `tag` | A tag, without `refs/tags/`. |
| `branch` | A branch, without `refs/heads/`. |
| `rev` | A commit id, full or short, or another revision that Git understands. |
| `path` | Optional. The note type file or directory inside the repository. Without it, janky uses `notetype.toml` at the top of the repository. |

A Git note type needs exactly one of `tag`, `rev` and `branch`. janky supports only public
repositories. It does not support SSH, passwords or local `file://` repositories.

`path` must stay inside the repository. A path such as `../up` gives the error `config::source`.

> [!WARNING]
> A `git://` URL has no encryption. Someone on the network can change the files that janky
> downloads. When janky fetches from a `git://` URL with no pinned commit, it gives the warning
> `source::insecure`. After the build, check the commit that `deck.lock` records. Use `https://`
> when you can.

### From an archive

An archive is one file that holds many files, such as a `.zip` file.

```toml
[notetypes.grammar]
url = "https://example.com/grammar-notetype.zip"
sha256 = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
path = "grammar"
```

| Key | Meaning |
|-----|---------|
| `url` | The URL of a `.tar.gz`, `.tgz` or `.zip` file. It must start with `https://`. Plain `http://` works only for your own computer (`localhost`, `127.0.0.1` or `[::1]`). |
| `sha256` | Required. The SHA-256 checksum of the archive: 64 lowercase hexadecimal characters. |
| `path` | Optional. The note type file or directory inside the archive. Without it, janky uses `notetype.toml` at the top of the archive. |

A checksum is a fingerprint of a file. janky compares the checksum of the downloaded file with
`sha256`. If they are not the same, janky stops with the error `source::archive`:

```text
source::archive
  x the archive has SHA-256 39348ad…, not 49348ad…
```

To get the checksum of an archive, download it and run `sha256sum` on it (on macOS,
`shasum -a 256`).

### What janky keeps from a source

janky keeps only the regular files and directories of a source. It skips links, Git submodules
and `.git` entries. An archive entry with `..` or an absolute path is an error. An archive larger
than 5,000,000,000 bytes is an error.

In diagnostics, janky shows a file of a remote note type as the key, `@`, the first 12 characters
of the commit or checksum, and the path. For example:

```text
config::missing-notetype
  x `grammar@39348ad22a6b/grammar/notetype.toml` does not exist
```

## The user cache

janky keeps downloaded sources in a cache for your user account, not in the project:

- `$XDG_CACHE_HOME/janky/sources/`, if the environment variable `XDG_CACHE_HOME` is set.
- Otherwise, `~/.cache/janky/sources/`.

All your projects share this cache. janky trusts the files in it and never changes a version that
is already there.

## Pinning in deck.lock

After a build, `deck.lock` records the exact version of each remote note type. For a Git source,
it records the commit. For an archive, it records the URL and the checksum:

```toml
source = { url = "http://127.0.0.1:18080/nt.tar.gz", sha256 = "3934…9bea" }
```

While the `git` URL and the `tag`, `rev` or `branch` in `deck.toml` are the same as in this
record, janky builds from the recorded commit. It downloads only if that commit is not in the
cache. So a `branch` does not move forward by itself. A new commit on the branch arrives only when
you run `janky update`.

If you change the URL or the reference in `deck.toml`, janky ignores the record. It finds the
version again at the next `check`, `plan` or `build`.

> [!NOTE]
> `check`, `plan` and `build` can use the network for a remote note type. They do this when no
> commit is pinned (the first build, or after you change the reference), or when the pinned
> version is not in the cache.

## Updating remote note types

`janky update` gets the newest commit for each `tag` and `branch`. It writes the new commits into
`deck.lock`. A `rev` keeps its commit, and an archive keeps its checksum.

```console
$ janky update
remote: ce4bec3f6b9d -> 152067e53c49
$ janky update
remote: unchanged at 152067e53c49
```

For each remote note type, `janky update` prints one of these lines:

| Line | Meaning |
|------|---------|
| `<key>: <old> -> <new>` | The note type moves to a new commit. |
| `<key>: unchanged at <commit>` | There is no new commit. |
| `` <key>: new, `janky build` records it `` | `deck.lock` has no record for it yet. |
| `No remote note types.` | The project has no remote note types. |

`janky update` writes only the source records of `deck.lock`. The other records change at the
next `janky build`. Run `janky plan` after an update to see how the new version changes your
notes.

## Working offline

`--offline` stops janky from downloading remote note types. janky then uses only the user cache.
(`janky schema` does not obey `--offline`.)
If a source is not in the cache, janky stops with an error:

```text
source::git
  x `https://example.invalid/x.git` is not in the cache. Run janky without --offline
```

With `janky update --offline`, janky makes a dry run from the cache and writes nothing:

```console
$ janky update --offline
Dry run with --offline: these are the changes that `janky update` would make, from the cached repositories. Commits that are not in the cache yet do not show.
remote: unchanged at ce4bec3f6b9d
```
