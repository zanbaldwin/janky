# Installation

There are no ready-made downloads of janky yet. You build the `janky` program from its source code.
This takes some time the first time, because janky includes parts of Anki itself.

janky has only been built and tested on Linux. The setup downloads a helper program that only runs on
x86-64 computers (64-bit Intel or AMD processors).

## What you need

Install these tools first:

| Tool | Why janky needs it |
|------|--------------------|
| [Rust](https://www.rust-lang.org/tools/install) (edition 2024) | The language that janky is written in. It includes `cargo`, the build tool. |
| [Git](https://git-scm.com) | Gets the source code of janky and of Anki. |
| A C compiler (for example `gcc`) | Compiles two small libraries that janky includes (Lua and SQLite). |
| [`just`](https://github.com/casey/just) | Runs the setup commands of the project. |
| `curl` and `unzip` | The setup uses them to download and unpack `protoc`. |

## Build janky

1. Get the janky source code (clone its Git repository).
2. Open a terminal in the directory of the source code.
3. Run the setup:

   ```console
   $ just setup
   ```

   This downloads a helper program (`protoc`) and a copy of the Anki source code into the
   source directory. You only do this once.

4. Build the program:

   ```console
   $ cargo build --release -p janky-cli
   ```

When the build is complete, the program is at `target/release/janky` in the source directory.

## Put janky on your PATH

Your PATH is the list of directories where the terminal looks for programs. If `janky` is in one of
these directories, you can run `janky` from any project directory.

1. Copy `target/release/janky` to a directory on your PATH (for example `~/.local/bin/`).
2. Open a new terminal.
3. Make sure that janky starts:

   ```console
   $ janky --version
   janky 0.1.0
   ```

If the terminal says that it cannot find `janky`, the directory is not on your PATH. Use a different
directory, or add the directory to your PATH.

## Next

Follow the [tutorial](tutorial/index.md) to make your first deck.
