//! The library behind the `janky` binary, which builds an Anki deck from the files of a project.
//!
//! A project is a directory with a `deck.toml`, note type files, content files and media. The
//! [`cli`] module runs the stages below in this order. Every stage collects its problems in
//! [`diagnostic::Diagnostics`] and continues where it can, so that one run reports all errors.
//! Stages 5 and 6 run only if stages 1 to 4 give no errors.
//!
//! 1. [`project`] loads `deck.toml` and the note types, local or remote ([`source`]), and checks
//!    how they fit together. [`config`] has the file types, and [`tree`] parses each file with
//!    the position of every value.
//! 2. [`plugin`] loads the Lua plugins into the sandbox of [`lua`].
//! 3. [`content`] reads the content files, checks them against the schema from [`schema`], and
//!    finds the note type of each entry.
//! 4. [`render`] renders the fields of each entry with minijinja, and collects the referenced
//!    media ([`media`]) and the generator jobs ([`generate`]).
//! 5. [`lock`] gives the stable ids from `deck.lock`, [`package`] makes a
//!    [`janky_anki::Package`], and [`plan`] compares the new lock file with the old one.
//! 6. [`janky_anki::write_apkg`] writes the `.apkg`, and the new `deck.lock` is written last.
//!
//! `janky preview` ([`preview`]) runs stages 1 to 5 on each page load, renders the cards with
//! [`janky_anki::render`], and serves them on `127.0.0.1`. It writes no files.
//!
//! `janky publish` ([`publish`]) applies the package to a copy of the user's collection and
//! syncs it with their sync server, through [`janky_anki::Session`].
//!
//! `janky generate` runs the generator jobs outside this pipeline: [`runner`] runs them
//! with the host functions of [`host`], network access through [`http`], secrets from
//! [`secrets`], and stores the results in the media cache or the data cache of [`cache`].
//!
//! The modules:
//!
//! - [`cache`]: the media cache and the data cache for generator results, and the plugin file area.
//! - [`cli`]: the commands, their output and the safe file writes.
//! - [`config`]: the serde types of `deck.toml` and note type files, and their schema-checked load.
//! - [`content`]: content files, `defaults`, and the entry type of each entry.
//! - [`diagnostic`]: errors and warnings, for the terminal or as JSON Lines.
//! - [`fs`]: `janky.fs`, read-only file access for plugins, also inside zip files.
//! - [`generate`]: generator jobs for media and data, and the hashes that name their results.
//! - [`host`]: the `janky` table of host functions for Lua, and value conversion.
//! - [`http`]: the HTTP client with allowed hosts, rate limits and retries.
//! - [`lock`]: `deck.lock` with ids, GUIDs, hashes and modification times.
//! - [`lua`]: the restricted Lua 5.4 state with memory, instruction and time limits.
//! - [`media`]: the lookup of referenced media in the media directories.
//! - [`package`]: the project, render output and lock file as a [`janky_anki::Package`].
//! - [`plan`]: what the next build changes, compared with the previous build.
//! - [`plugin`]: loading plugins, and their filters, functions and generators in templates.
//! - [`plugin_cache`]: `janky.cache`, one SQLite file for each plugin instance.
//! - [`preview`]: `janky preview`, a local web server that shows each card in the browser.
//! - [`project`]: `deck.toml` and the note types, and the checks between them.
//! - [`publish`]: `janky publish`, through a copy of the collection and the sync server.
//! - [`render`]: field rendering and the card template checks.
//! - [`runner`]: runs generator jobs as coroutines, with limits and an HTTP interface.
//! - [`schema`]: the `[schema]` short form compiled to JSON Schema.
//! - [`secrets`]: plugin secrets from environment variables, files and `.env`.
//! - [`show`]: `janky show`, the cards of one entry as text or JSON, for agents and scripts.
//! - [`source`]: remote note types from Git or archive URLs, with a local cache.
//! - [`tree`]: YAML, TOML and JSON parsed to a value with a JSON Pointer to span map.

pub mod cache;
pub mod cli;
pub mod config;
pub mod content;
pub mod diagnostic;
pub mod fs;
pub mod generate;
pub mod host;
pub mod http;
pub mod lock;
pub mod lua;
pub mod media;
pub mod package;
pub mod plan;
pub mod plugin;
pub mod plugin_cache;
pub mod preview;
pub mod project;
pub mod publish;
pub mod render;
pub mod runner;
pub mod schema;
pub mod secrets;
pub mod show;
pub mod source;
pub mod tree;

pub use cli::Cli;
