# Using the command line

You use janky from a terminal. Each command starts with `janky`, then the name of the command,
for example `janky check`. Run the commands in the project directory (the directory with
`deck.toml`), or tell janky where the project is with `--deck`.

janky writes a package (an `.apkg` file), and you import that package into Anki yourself. If you
run your own Anki sync server, `janky publish` can put the project into your collection instead. This part explains the commands, the messages that janky prints and
the import options in Anki.

- [The everyday workflow](workflow.md): the order in which you run the commands, and how to keep
  your project in Git.
- [Command reference](commands.md): each command, its flags, its output and its exit codes.
- [Reading errors and warnings](diagnostics.md): how to read a diagnostic in the terminal and in
  JSON form.
- [Importing into Anki](importing.md): the import options to use, and what happens in Anki for
  each kind of change.
- [Publishing through a sync server](publishing.md): `janky publish` and `janky pull`.
