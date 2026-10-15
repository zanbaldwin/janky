//! The `janky` commands, their output, and the safe file writes.
//!
//! [`Cli`] is the clap parser, and its doc comments are the `--help` text. In short:
//!
//! - `check` loads and renders everything and writes no files.
//! - `plan` also builds the package in memory and shows what changes against `deck.lock`.
//! - `build` shows the plan, then writes the `.apkg` and the new `deck.lock`.
//! - `schema` writes the JSON Schemas for editors to `.janky/schema/`, and prints the editor
//!   settings that use them.
//! - `update` moves the remote note types in `deck.lock` to their newest commit or hash.
//! - `preview` serves the cards of the project to the browser on `127.0.0.1`, and writes no files
//!   (see [`crate::preview`]).
//! - `show` prints the cards of one entry as text or JSON, and writes no files (see [`crate::show`]).
//! - `publish` puts the project into the user's collection through their sync server, and
//!   `pull` gets missing generated media back from it (see [`crate::publish`]).
//! - `generate` runs the generator jobs, media and data, whose results are not in the cache yet.
//!   `prune` removes cached results that no job uses, and old temporary files. It refuses while
//!   a job is pending. Both take the lock [`LOCK_FILE`] before they load the project, and ask
//!   before they change the cache, unless `--yes` is set.
//!
//! The global flags: `--deck` is the path to `deck.toml` (default `deck.toml`). `--quiet`
//! hides the messages and the plugin log, but not the diagnostics. Without `--verbose`, the
//! info diagnostics show as one line for each code. `--json` writes the
//! diagnostics as JSON Lines on stdout, and for `plan` and `build` one more line with the plan.
//! `--offline` uses only cached remote note types (for `update`, it makes a dry run).
//! `--allow-missing` lets `build` continue when generated
//! results are missing, and lets `prune` run while jobs are pending.
//!
//! [`check`] and [`prepare`] are the shared stages of the commands, and [`run`] dispatches.
//! [`check`] loads the project and the plugins, then loads the content and renders it.
//! [`prepare`] runs [`check`], stops on any error, reads `deck.lock`, then makes the package and
//! the plan. The `check` command uses only [`check`]. `plan` and `build` use [`prepare`], and
//! `build` then writes the `.apkg` first and `deck.lock` second, so the pair is not atomic.
//! `generate` and `prune` use [`check`]. `schema` loads the project and the plugins, but no
//! content. `update` loads only the project.
//!
//! Plugin log lines print first, each with a `plugin: ` prefix, then the messages.
//! Text diagnostics and messages go to stderr. The plan text of `plan` goes to stdout.
//!
//! janky writes only inside the project directory and never through a symbolic link. The lock
//! file is first written to `deck.lock.tmp` and then renamed. If that file is still there, a
//! previous run stopped early, and `build` and `update` refuse to continue.

use std::fmt::Write as _;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use clap::{Parser, Subcommand};
use janky_anki::Package;
use miette::{GraphicalReportHandler, GraphicalTheme};
use serde_json::Value;

use crate::config::deck::{Deck, DeckFile};
use crate::config::notetype::{NotetypeFile, ShortSchema};
use crate::content::{self, ContentFile};
use crate::diagnostic::{Diagnostic, Diagnostics, json_text, visible};
use crate::lock::{self, LockFile};
use crate::plan::{self, Plan};
use crate::plugin::{self, Plugins};
use crate::project::{self, Project};
use crate::render::{self, Rendered};
use crate::{config, package, schema};

const DEFAULT_OUTPUT: &str = ".janky/build/deck.apkg";
const SCHEMA_DIR: &str = ".janky/schema";

/// Build Anki decks from plain files.
///
/// janky reads deck.toml, the note types and the content files, and writes an .apkg package
/// that Anki imports. deck.lock keeps the Anki ids, so a new import updates the notes that
/// changed and keeps their review history.
#[derive(Debug, Parser)]
#[command(name = "janky", version)]
pub struct Cli {
    /// Path to deck.toml; the project directory is its parent.
    #[arg(long = "deck", global = true, default_value = "deck.toml", value_name = "deck.toml")]
    pub project: PathBuf,
    /// Hide the messages and the plugin log; diagnostics still show.
    #[arg(long, global = true)]
    pub quiet: bool,
    /// Show info diagnostics in full; without it, each info code shows as one line.
    #[arg(long, global = true)]
    pub verbose: bool,
    /// Write the diagnostics as JSON Lines on stdout.
    ///
    /// plan and build also write one more line with the plan and the pending generator jobs.
    #[arg(long, global = true)]
    pub json: bool,
    /// Use only the cached remote note types; do not fetch.
    ///
    /// With update, show the changes that it would make from the cache, and write nothing.
    #[arg(long, global = true)]
    pub offline: bool,
    /// Continue when generated media or data is missing.
    ///
    /// build writes the package without the missing results, and prune runs while jobs are
    /// pending. After janky generate, the next build updates the affected notes.
    #[arg(long, global = true)]
    pub allow_missing: bool,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Check the whole project and write nothing.
    ///
    /// Loads deck.toml, the note types, the plugins and the content, renders every field and
    /// checks the card templates and media references. Missing generated results are warnings.
    Check,
    /// Show what the next build adds, changes or removes, and write nothing.
    ///
    /// Runs check, then makes the package in memory and compares it with deck.lock. The plan
    /// goes to stdout, with the generator jobs that are still pending.
    Plan,
    /// Write the .apkg package and the new deck.lock.
    ///
    /// Shows the plan first. Stops when generated results are missing, unless --allow-missing
    /// is set. The package goes to .janky/build/deck.apkg, or to the output key of [deck] in
    /// deck.toml.
    Build,
    /// Write JSON Schemas for editors to .janky/schema/ and print the editor settings.
    ///
    /// The settings are for Taplo (TOML) and VS Code (YAML and JSON content).
    Schema,
    /// Move the remote note types in deck.lock to their newest commit or hash.
    ///
    /// With --offline, show the changes from the cached repositories and write nothing.
    Update,
    /// Run the generator jobs (media and data) whose results are not in the cache.
    ///
    /// Shows the jobs, their network hosts and their secrets, then asks before it runs them.
    /// Only one generate or prune can run in a project at a time.
    Generate {
        /// Run without asking; required when stdin is not a terminal.
        #[arg(long)]
        yes: bool,
    },
    /// Remove cached results that no job uses, and old temporary files.
    ///
    /// Refuses while a job is pending (see --allow-missing), because a missing result can hide
    /// the jobs that depend on it. Asks before it removes files.
    Prune {
        /// Remove without asking; required when stdin is not a terminal.
        #[arg(long)]
        yes: bool,
    },
    /// Put the project into your Anki collection through your sync server.
    ///
    /// Syncs a copy of the collection in .janky/publish/, shows what changes, asks, applies the
    /// project and syncs again. Unlike an .apkg import, it updates presets, renames decks and
    /// moves the cards of moved notes. Needs [publish] in deck.toml.
    Publish {
        /// Publish without asking; required when stdin is not a terminal.
        #[arg(long)]
        yes: bool,
        /// Show what would change, and change nothing on the server.
        #[arg(long)]
        dry_run: bool,
        /// Allow a schema change, which replaces the collection on the server.
        ///
        /// Sync your other devices first: their reviews that the server does not have are lost.
        /// Do not review on another device while janky publish runs, because the upload replaces
        /// the reviews that reach the server in that time.
        #[arg(long)]
        allow_full_sync: bool,
        /// Delete the notes of entries that the project removed, with their cards.
        #[arg(long)]
        delete_removed: bool,
    },
    /// Show every card of the project in the browser, and write nothing.
    ///
    /// Starts a web server on 127.0.0.1 that renders each card with Anki's template renderer.
    /// Reload the page to read the files again. Generators do not run. Ctrl+C stops the server.
    Preview {
        /// The TCP port on 127.0.0.1. 0 lets the operating system choose a port.
        #[arg(long, default_value_t = 5259)]
        port: u16,
    },
    /// Print the cards of one entry as text, or as JSON with --json, and write nothing.
    ///
    /// The identifier is <entry> or <entry>/<card>. Without it, show lists the identifiers. With
    /// --sample, the identifier names a note type, and show prints its sample note.
    Show {
        /// <entry> or <entry>/<card>. With --sample: <notetype> or <notetype>/<card>.
        identifier: Option<String>,
        /// Print only one side.
        #[arg(long, value_enum)]
        side: Option<crate::show::Side>,
        /// The typed answer, for a back with {{type:Field}}.
        #[arg(long, default_value = "")]
        typed: String,
        /// The identifier names a note type, and show prints its sample note.
        #[arg(long)]
        sample: bool,
        /// text, or html: one side of one card as a document for a file:/// URL. --json has priority.
        #[arg(long, value_enum, default_value_t)]
        format: crate::show::Format,
        /// The night mode classes on the card, for --format html.
        #[arg(long)]
        night: bool,
        /// The platform classes on the card, for --format html.
        #[arg(long, value_enum, default_value_t)]
        platform: crate::preview::Platform,
    },
    /// Download the missing generated media files from the sync server into the cache.
    ///
    /// For a fresh clone: janky looks for the files of the pending media jobs on the server,
    /// and so does not pay for them again. Each file must match deck.lock. Data results are not
    /// on the server. Needs [publish] in deck.toml.
    Pull,
}

/// The result of [`check`]: the loaded project and plugins, the content files and the render
/// output.
pub struct Checked {
    pub project: Project,
    pub plugins: Plugins,
    pub files: Vec<ContentFile>,
    pub rendered: Rendered,
}

/// Loads the project, the plugins and the content files, and renders every entry.
///
/// Returns `None` only when the project or the plugins cannot load. Otherwise it returns a
/// [`Checked`], and the errors are in `diagnostics`. The plugin log lines go to `log`.
pub fn check(
    path: &Path,
    options: project::Options,
    diagnostics: &mut Diagnostics,
    log: &mut Vec<String>,
) -> Option<Checked> {
    let project = project::load_with(path, options, diagnostics)?;
    let plugins = plugin::load(&project, diagnostics)?;
    plugin::check_variables(&project, &plugins, diagnostics);
    let files = content::load(&project, diagnostics);
    let rendered = render::render_with(&project, Some(&plugins), &files, diagnostics);
    log.extend(rendered.log.iter().cloned());
    Some(Checked { project, plugins, files, rendered })
}

/// The result of [`prepare`]: everything that `build` writes, and the plan.
pub struct Prepared {
    pub checked: Checked,
    /// The lock file on disk, or a new one if there is no `deck.lock`.
    pub previous: LockFile,
    /// The lock file that `build` writes.
    pub next: LockFile,
    pub package: Package,
    pub plan: Plan,
}

/// Runs [`check`], then makes the package and the plan in memory. It writes no files.
///
/// Returns `None` if [`check`] gives an error, or if the lock file or the package fails.
/// `deck.lock` is read from the project directory.
pub fn prepare(
    path: &Path,
    options: project::Options,
    diagnostics: &mut Diagnostics,
    log: &mut Vec<String>,
) -> Option<Prepared> {
    let checked = check(path, options, diagnostics, log)?;
    if diagnostics.has_errors() {
        return None;
    }
    let previous = lock::read(&checked.project.root.join("deck.lock"), diagnostics)?;
    let mut next = previous.clone();
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |time| time.as_secs() as i64);
    let package = package::package(&checked.project, &checked.files, &checked.rendered, &mut next, now, diagnostics)?;
    let plan = plan::plan(&previous, &next);
    Some(Prepared { checked, previous, next, package, plan })
}

/// Runs one command and prints its diagnostics and messages.
///
/// Returns failure when any diagnostic is an error.
pub fn run(cli: &Cli) -> ExitCode {
    let options = project::Options { offline: cli.offline, update: false };
    let mut diagnostics = Diagnostics::default();
    let mut messages = Vec::new();
    let mut log = Vec::new();
    let mut shown = None;
    let mut published = None;
    let mut printed = None;
    match &cli.command {
        Command::Check => {
            if let Some(checked) = check(&cli.project, options, &mut diagnostics, &mut log) {
                missing_media(&checked.rendered, false, &mut diagnostics);
                if !diagnostics.has_errors() {
                    messages.push(format!("{} notes are valid", checked.rendered.notes.len()));
                }
            }
        },
        Command::Plan => {
            if let Some(prepared) = prepare(&cli.project, options, &mut diagnostics, &mut log) {
                missing_media(&prepared.checked.rendered, false, &mut diagnostics);
                shown = Some(prepared);
            }
        },
        Command::Build => {
            if let Some(prepared) = prepare(&cli.project, options, &mut diagnostics, &mut log) {
                missing_media(&prepared.checked.rendered, !cli.allow_missing, &mut diagnostics);
                if !diagnostics.has_errors() {
                    messages.push(prepared.plan.text(&prepared.previous, &prepared.next));
                    messages.extend(pending_text(&prepared.checked.rendered.jobs));
                    build(&prepared, &mut diagnostics, &mut messages);
                }
                shown = Some(prepared);
            }
        },
        Command::Schema => write_schemas(&cli.project, &mut diagnostics, &mut messages),
        Command::Update => update(&cli.project, cli.offline, &mut diagnostics, &mut messages),
        Command::Generate { yes } => generate(cli, *yes, options, &mut diagnostics, &mut messages, &mut log),
        Command::Prune { yes } => prune(cli, *yes, options, &mut diagnostics, &mut messages, &mut log),
        Command::Show {
            identifier,
            side,
            typed,
            sample,
            format,
            night,
            platform,
        } => {
            let request = crate::show::Request {
                identifier: identifier.as_deref(),
                side: *side,
                typed,
                sample: *sample,
                json: cli.json,
                format: *format,
                night: *night,
                platform: *platform,
            };
            printed = crate::show::show(&cli.project, options, &request, &mut diagnostics);
        },
        Command::Preview { port } => crate::preview::serve(&cli.project, options, *port, &mut diagnostics),
        Command::Pull => crate::publish::pull_media(cli, options, &mut diagnostics, &mut messages, &mut log),
        Command::Publish {
            yes,
            dry_run,
            allow_full_sync,
            delete_removed,
        } => {
            let flags = crate::publish::Flags {
                yes: *yes,
                dry_run: *dry_run,
                allow_full_sync: *allow_full_sync,
                delete_removed: *delete_removed,
            };
            published = crate::publish::publish(cli, flags, options, &mut diagnostics, &mut messages, &mut log);
        },
    }
    let plugin: Vec<String> =
        log.iter().flat_map(|line| line.split('\n')).map(|part| format!("plugin: {part}")).collect();
    let count = plugin.len();
    let mut messages: Vec<String> = plugin.into_iter().chain(messages).collect();
    if messages.len() > count && count > 0 {
        messages.insert(count, String::new());
    }
    report(cli, &diagnostics, &messages);
    if cli.json
        && let Some(json) = published
    {
        println!("{}", json_text(&json));
    }
    if let Some(text) = printed {
        print!("{text}");
    }
    if let Some(prepared) = shown {
        if cli.json {
            let mut json = prepared.plan.to_json();
            json["plan"]["pending_jobs"] = serde_json::json!(
                crate::generate::pending(&prepared.checked.rendered.jobs)
                    .into_iter()
                    .map(|job| serde_json::json!({ "instance": job.instance, "input": job.input, "units": job.units, "unit": job.unit }))
                    .collect::<Vec<_>>()
            );
            println!("{}", json_text(&json));
        } else if matches!(cli.command, Command::Plan) {
            print!("{}", visible(&prepared.plan.text(&prepared.previous, &prepared.next)));
            if let Some(text) = pending_text(&prepared.checked.rendered.jobs) {
                print!("{}", visible(&text));
            }
        }
    }
    if diagnostics.has_errors() { ExitCode::FAILURE } else { ExitCode::SUCCESS }
}

fn short(input: &str) -> String {
    let input: String = input.chars().map(|c| if c.is_whitespace() { ' ' } else { c }).collect();
    let mut shown: String = input.chars().take(60).collect();
    if input.chars().count() > 60 {
        shown.push('…');
    }
    shown
}

/// Asks `question` on the terminal. Without a terminal, it pushes the error `code` with
/// `message`, unless `yes` is set.
pub(crate) fn confirm(question: &str, yes: bool, code: &str, message: &str, diagnostics: &mut Diagnostics) -> bool {
    use std::io::{BufRead, IsTerminal};
    if yes {
        return true;
    }
    if !std::io::stdin().is_terminal() {
        diagnostics.push(Diagnostic::error(code, message).with_help("run again with `--yes`"));
        return false;
    }
    let _ = write!(std::io::stderr(), "{question} [y/N] ");
    let mut answer = String::new();
    let _ = std::io::stdin().lock().read_line(&mut answer);
    matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

fn source_text(source: &crate::config::deck::SecretSource) -> String {
    match (&source.env, &source.file) {
        (Some(env), Some(file)) => format!("environment variable `{env}`, then `{file}`, then `.env`"),
        (Some(env), None) => format!("environment variable `{env}`, then `.env`"),
        (None, Some(file)) => format!("file `{file}`"),
        (None, None) => String::new(),
    }
}

fn summary(checked: &Checked, pending: &[crate::generate::Job]) -> String {
    let mut out = pending_text(pending).unwrap_or_default();
    let mut instances: Vec<&str> = pending.iter().map(|job| job.instance.as_str()).collect();
    instances.dedup();
    instances.sort();
    instances.dedup();
    for name in instances {
        let count = pending.iter().filter(|job| job.instance == name).count();
        let _ = writeln!(out, "{name}: {count} jobs");
        let Some(instance) = checked.project.deck.plugins.get(name) else { continue };
        match &instance.http {
            Some(http) if !http.allowed_hosts.is_empty() => {
                let _ = writeln!(out, "  network: {}", http.allowed_hosts.join(", "));
            },
            _ => {
                let _ = writeln!(out, "  network: none");
            },
        }
        for (secret, source) in &instance.secrets {
            let _ = writeln!(out, "  secret `{secret}`: {}", source_text(source));
        }
    }
    out
}

fn generate(
    cli: &Cli,
    yes: bool,
    options: project::Options,
    diagnostics: &mut Diagnostics,
    messages: &mut Vec<String>,
    log: &mut Vec<String>,
) {
    use crate::runner::Outcome;
    let Some(_lock) = lock_project(&cli.project, diagnostics) else {
        return;
    };
    let Some(checked) = check(&cli.project, options, diagnostics, log) else {
        return;
    };
    if diagnostics.has_errors() {
        return;
    }
    let pending: Vec<crate::generate::Job> =
        crate::generate::pending(&checked.rendered.jobs).into_iter().cloned().collect();
    if pending.is_empty() {
        messages.push("All generator results are in the cache.".into());
        return;
    }
    let secrets = crate::secrets::resolve(&checked.project, &checked.plugins, diagnostics);
    if diagnostics.has_errors() {
        return;
    }
    let _ = write!(std::io::stderr(), "{}", visible(&summary(&checked, &pending)));
    if !confirm(&format!("Run {} jobs?", pending.len()), yes, "generate::confirm", CACHE_QUESTION, diagnostics) {
        if !diagnostics.has_errors() {
            messages.push("Nothing was generated.".into());
        }
        return;
    }
    let client = match crate::http::Client::new(&checked.project, secrets) {
        Ok(client) => client,
        Err(message) => {
            diagnostics.push(Diagnostic::error("generate::client", message));
            return;
        },
    };
    let limits = crate::runner::limits(&checked.project);
    let total = pending.len();
    let mut index = 0;
    let quiet = cli.quiet;
    let results = crate::runner::run(
        &checked.plugins,
        &checked.project.root,
        pending,
        &limits,
        std::sync::Arc::new(client),
        |instance| {
            if !quiet {
                let _ = writeln!(std::io::stderr(), "{}", visible(&format!("{instance}: running `prepare`")));
            }
        },
        |job, outcome| {
            index += 1;
            let state = match outcome {
                Outcome::Stored { file, .. } => format!("wrote {file}"),
                Outcome::Refused(_) => "refused".into(),
                Outcome::Unauthorised { .. } => "unauthorised".into(),
                Outcome::Failed(_) => "failed".into(),
                Outcome::Skipped => "not started".into(),
            };
            if !quiet {
                let line = format!("[{index}/{total}] {}: {}: {state}", job.instance, short(&job.input_text()));
                let _ = writeln!(std::io::stderr(), "{}", visible(&line));
            }
            !matches!(outcome, Outcome::Unauthorised { .. })
        },
    );
    log.extend(checked.plugins.sandbox.take_log());
    let mut counts = [0usize; 4];
    let mut costs: std::collections::BTreeMap<String, f64> = std::collections::BTreeMap::new();
    for (job, outcome) in results {
        let input = short(&job.input_text());
        match outcome {
            Outcome::Stored { cost, .. } => {
                counts[0] += 1;
                if let Some(cost) = cost {
                    *costs.entry(job.instance.clone()).or_default() += cost;
                }
            },
            Outcome::Refused(message) => {
                counts[1] += 1;
                diagnostics.push(Diagnostic::error(
                    "generate::refused",
                    format!("{} refused `{input}`: {message}", job.instance),
                ));
            },
            Outcome::Unauthorised { message, secrets } => {
                counts[1] += 1;
                let names: Vec<String> = secrets.iter().map(|name| format!("`{name}`")).collect();
                diagnostics.push(
                    Diagnostic::error(
                        "generate::unauthorised",
                        format!(
                            "{}: the server refused the secret {} for `{input}`: {message}",
                            job.instance,
                            names.join(", ")
                        ),
                    )
                    .with_help("check the secret's value and the account's quota; janky stopped the run"),
                );
            },
            Outcome::Failed(failure) => {
                counts[2] += 1;
                diagnostics.push(failure.with_help(format!("job: {} `{input}`", job.instance)));
            },
            Outcome::Skipped => counts[3] += 1,
        }
    }
    messages.push(format!(
        "Generated {} results: {} refused, {} failed, {} not started",
        counts[0], counts[1], counts[2], counts[3]
    ));
    for (instance, cost) in costs.into_iter().filter(|(_, cost)| cost.is_finite()) {
        let cost = if cost.fract() == 0.0 && cost.abs() < 1e15 { format!("{cost:.0}") } else { format!("{cost:.2}") };
        messages.push(format!("Reported cost for {instance}: {cost}"));
    }
}

/// The lock file of the commands that change the caches, relative to the project root.
pub const LOCK_FILE: &str = ".janky/generate.lock";

const CACHE_QUESTION: &str = "janky asks before it changes the cache, and stdin is not a terminal";

/// Takes the project's lock for `generate` and `prune`, before the project is
/// rendered, so that a second run cannot plan the same paid jobs. The lock lasts while the
/// returned file is open, and the operating system releases it when janky ends, also after a
/// crash.
pub(crate) fn lock_project(project: &Path, diagnostics: &mut Diagnostics) -> Option<std::fs::File> {
    let root = project.parent().unwrap_or(Path::new(""));
    let path = root.join(LOCK_FILE);
    let opened = prepare_target(root, &path).and_then(|()| {
        std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)
            .map_err(|error| error.to_string())
    });
    let file = match opened {
        Ok(file) => file,
        Err(message) => {
            diagnostics
                .push(Diagnostic::error("generate::lock", format!("cannot open `{}`: {message}", path.display())));
            return None;
        },
    };
    match file.try_lock() {
        Ok(()) => Some(file),
        Err(std::fs::TryLockError::WouldBlock) => {
            diagnostics.push(
                Diagnostic::error(
                    "generate::locked",
                    "another `janky generate`, `janky prune` or `janky pull` is running for this project",
                )
                .with_help("wait until it ends, then run again"),
            );
            None
        },
        Err(std::fs::TryLockError::Error(error)) => {
            diagnostics.push(Diagnostic::error("generate::lock", format!("cannot lock `{}`: {error}", path.display())));
            None
        },
    }
}

fn prune(
    cli: &Cli,
    yes: bool,
    options: project::Options,
    diagnostics: &mut Diagnostics,
    messages: &mut Vec<String>,
    log: &mut Vec<String>,
) {
    let Some(_lock) = lock_project(&cli.project, diagnostics) else {
        return;
    };
    let Some(checked) = check(&cli.project, options, diagnostics, log) else {
        return;
    };
    if diagnostics.has_errors() {
        return;
    }
    let pending: Vec<&crate::generate::Job> = checked.rendered.jobs.iter().filter(|job| !job.exists).collect();
    if !pending.is_empty() && !cli.allow_missing {
        let mut names: Vec<String> =
            pending.iter().take(5).map(|job| format!("{} `{}`", job.instance, short(&job.input_text()))).collect();
        if pending.len() > 5 {
            names.push(format!("{} more", pending.len() - 5));
        }
        diagnostics.push(
            Diagnostic::error(
                "prune::pending",
                format!(
                    "{} generated results are missing, so janky cannot see every job of the project",
                    pending.len()
                ),
            )
            .with_help(format!(
                "run `janky generate` first, or prune with `--allow-missing`. Pending: {}",
                names.join(", ")
            )),
        );
        return;
    }
    let root = &checked.project.root;
    let used: std::collections::HashSet<&str> = checked.rendered.jobs.iter().map(|job| job.hash.as_str()).collect();
    let mut unused = Vec::new();
    for dir in [crate::cache::media_dir(root), crate::cache::data_dir(root)].into_iter().flatten() {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let regular = entry.file_type().is_ok_and(|kind| kind.is_file());
            let old = entry
                .metadata()
                .and_then(|metadata| metadata.modified())
                .is_ok_and(|modified| modified.elapsed().is_ok_and(|age| age > std::time::Duration::from_secs(3600)));
            let temporary = name.starts_with('.') && name.contains(".tmp-") && old;
            let stem = name.split('.').next().unwrap_or_default();
            let cached = !stem.is_empty() && stem.bytes().all(|byte| byte.is_ascii_hexdigit()) && name.contains('.');
            if regular && (temporary || (cached && !used.contains(stem))) {
                unused.push(entry.path());
            }
        }
    }
    unused.sort();
    if unused.is_empty() {
        messages.push("Nothing to prune.".into());
        return;
    }
    let bytes: u64 = unused.iter().filter_map(|path| std::fs::metadata(path).ok()).map(|metadata| metadata.len()).sum();
    let _ = writeln!(std::io::stderr(), "{} cached files are not used by any job ({bytes} bytes).", unused.len());
    if !confirm("Remove them?", yes, "generate::confirm", CACHE_QUESTION, diagnostics) {
        if !diagnostics.has_errors() {
            messages.push("Nothing was removed.".into());
        }
        return;
    }
    let mut removed = 0;
    for path in &unused {
        match std::fs::remove_file(path) {
            Ok(()) => removed += 1,
            Err(error) => diagnostics
                .push(Diagnostic::error("prune::remove", format!("cannot remove `{}`: {error}", path.display()))),
        }
    }
    messages.push(format!("Removed {removed} files ({bytes} bytes)."));
}

fn pending_text(jobs: &[crate::generate::Job]) -> Option<String> {
    let pending = crate::generate::pending(jobs);
    if pending.is_empty() {
        return None;
    }
    let mut units = std::collections::BTreeMap::new();
    for job in &pending {
        if let (Some(count), Some(unit)) = (job.units, &job.unit) {
            *units.entry(unit.clone()).or_insert(0u64) += count;
        }
    }
    let estimate = if units.is_empty() {
        String::new()
    } else {
        let parts: Vec<String> = units.iter().map(|(unit, count)| format!("{count} {unit}")).collect();
        format!(" ({})", parts.join(", "))
    };
    let mut out = format!("Generator jobs: {} to run{estimate}\n", pending.len());
    for job in pending {
        let verb = if job.kind == crate::generate::Kind::Data { "enrich" } else { "media" };
        out.push_str(&format!("+ {verb} {}: {}\n", job.instance, short(&job.input_text())));
    }
    Some(out)
}

pub(crate) fn missing_media(rendered: &Rendered, strict: bool, diagnostics: &mut Diagnostics) {
    use crate::generate::Kind;
    for (kind, code, what) in [
        (Kind::Media, "generate::missing", "generated media files are missing"),
        (Kind::Data, "enrich::missing", "enrichment results are missing"),
    ] {
        let missing = rendered.jobs.iter().filter(|job| job.kind == kind && !job.exists).count();
        if missing == 0 {
            continue;
        }
        let message = format!("{missing} {what}");
        let help = "run `janky generate`";
        diagnostics.push(if strict {
            Diagnostic::error(code, message).with_help(format!("{help}, or build with `--allow-missing`"))
        } else {
            Diagnostic::warning(code, message).with_help(help)
        });
    }
}

fn report(cli: &Cli, diagnostics: &Diagnostics, messages: &[String]) {
    if cli.json {
        print!("{}", diagnostics.to_json_lines());
    } else if !diagnostics.is_empty() {
        let handler = GraphicalReportHandler::new_themed(GraphicalTheme::default());
        if let Ok(text) = diagnostics.render(&handler, cli.verbose) {
            eprint!("{text}");
        }
    }
    if !cli.quiet {
        let mut stderr = std::io::stderr();
        for message in messages {
            let _ = writeln!(stderr, "{}", visible(message));
        }
    }
}

fn build(prepared: &Prepared, diagnostics: &mut Diagnostics, messages: &mut Vec<String>) {
    let Prepared { checked, next: lock, package, .. } = prepared;
    let root = &checked.project.root;
    let lock_path = root.join("deck.lock");
    let deck = &checked.project.deck;
    let output = root.join(deck.deck.output.as_deref().unwrap_or(DEFAULT_OUTPUT));
    let temporary = lock_temporary(&lock_path);
    if std::fs::symlink_metadata(&temporary).is_ok() {
        diagnostics.push(
            Diagnostic::error("build::lock", format!("`{}` already exists", temporary.display()))
                .with_help("a previous build stopped early; remove the file and build again"),
        );
        return;
    }
    for target in [&output, &lock_path] {
        if let Err(error) = prepare_target(root, target) {
            diagnostics
                .push(Diagnostic::error("build::write", format!("cannot write `{}`: {error}", target.display())));
            return;
        }
    }
    let count = match janky_anki::write_apkg(package, &output) {
        Ok(count) => count,
        Err(error) => {
            diagnostics
                .push(Diagnostic::error("build::write", format!("cannot write `{}`: {error}", output.display())));
            return;
        },
    };
    if let Err(error) = write_lock(&lock_path, lock) {
        diagnostics.push(Diagnostic::error("build::lock", format!("cannot write `{}`: {error}", lock_path.display())));
        return;
    }
    let media = match package.media.len() {
        1 => "1 media file".to_string(),
        count => format!("{count} media files"),
    };
    messages.push(format!("wrote {}: {count} notes, {media}", output.display()));
    messages.push("In Anki, turn FSRS on. A package cannot change collection settings.".into());
}

fn update(path: &Path, offline: bool, diagnostics: &mut Diagnostics, messages: &mut Vec<String>) {
    let options = project::Options { offline, update: true };
    let Some(project) = project::load_with(path, options, diagnostics) else {
        return;
    };
    let lock_path = project.root.join("deck.lock");
    let Some(mut lock) = lock::read(&lock_path, diagnostics) else { return };
    if offline {
        messages.push(
            "Dry run with --offline: these are the changes that `janky update` would make, from the cached \
             repositories. Commits that are not in the cache yet do not show."
                .into(),
        );
    }
    for (key, source) in &project.sources {
        let Some(locked) = lock.notetypes.get_mut(key) else {
            messages.push(format!("{key}: new, `janky build` records it"));
            continue;
        };
        let id = |source: &crate::lock::LockedSource| source.commit.clone().or_else(|| source.sha256.clone());
        let before = locked.source.as_ref().and_then(id);
        let after = id(source);
        let short =
            |value: &Option<String>| value.as_deref().map_or("none".to_string(), |value| value[..12].to_string());
        if before == after {
            messages.push(format!("{key}: unchanged at {}", short(&after)));
        } else {
            messages.push(format!("{key}: {} -> {}", short(&before), short(&after)));
        }
        locked.source = Some(source.clone());
    }
    if project.sources.is_empty() {
        messages.push("No remote note types.".into());
        return;
    }
    if offline {
        return;
    }
    let temporary = lock_temporary(&lock_path);
    if std::fs::symlink_metadata(&temporary).is_ok() {
        diagnostics.push(Diagnostic::error("update::lock", format!("`{}` already exists", temporary.display())));
        return;
    }
    if let Err(error) = prepare_target(&project.root, &lock_path)
        .map_err(std::io::Error::other)
        .and_then(|()| write_lock(&lock_path, &lock))
    {
        diagnostics.push(Diagnostic::error("update::lock", format!("cannot write `{}`: {error}", lock_path.display())));
    }
}

pub(crate) fn write_lock(path: &Path, lock: &LockFile) -> std::io::Result<()> {
    let temporary = lock_temporary(path);
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&temporary)?;
    file.write_all(lock.to_toml().as_bytes())?;
    drop(file);
    std::fs::rename(&temporary, path)
}

fn lock_temporary(path: &Path) -> PathBuf {
    path.with_extension("lock.tmp")
}

/// Makes sure that `target` can be written: its directory is inside `root`, and `target` is not
/// a symbolic link. Makes the missing directories.
pub(crate) fn prepare_target(root: &Path, target: &Path) -> Result<(), String> {
    let root = if root.as_os_str().is_empty() { Path::new(".") } else { root };
    let root = root.canonicalize().map_err(|error| error.to_string())?;
    let parent = target.parent().filter(|parent| !parent.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let inside = |path: &Path| path.canonicalize().is_ok_and(|path| path.starts_with(&root));
    let existing = parent.ancestors().find(|ancestor| ancestor.exists()).unwrap_or(Path::new("."));
    if !inside(existing) {
        return Err("the directory is outside the project directory".into());
    }
    std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    if !inside(parent) {
        return Err("the directory is outside the project directory".into());
    }
    if std::fs::symlink_metadata(target).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err("the file is a symbolic link".into());
    }
    Ok(())
}

fn write_schemas(path: &Path, diagnostics: &mut Diagnostics, messages: &mut Vec<String>) {
    let root = path.parent().unwrap_or(Path::new(""));
    let dir = root.join(SCHEMA_DIR);
    let write = |name: &str, value: &Value| {
        let file = dir.join(name);
        prepare_target(root, &file)
            .and_then(|()| {
                std::fs::write(&file, serde_json::to_string_pretty(value).unwrap_or_default() + "\n")
                    .map_err(|error| error.to_string())
            })
            .map(|()| format!("wrote {}", file.display()))
            .map_err(|error| format!("cannot write `{}`: {error}", file.display()))
    };
    let project = project::load(path, diagnostics);
    let mut schemas = vec![
        ("deck.schema.json", deck_schema(None)),
        ("notetype.schema.json", config::schema_for::<NotetypeFile>()),
        ("lock.schema.json", config::schema_for::<LockFile>()),
    ];
    if let Some(project) = &project {
        schemas.push(("content.schema.json", schema::file_schema(project)));
    }
    for (name, value) in &schemas {
        match write(name, value) {
            Ok(message) => messages.push(message),
            Err(message) => {
                diagnostics.push(Diagnostic::error("schema::write", message));
                return;
            },
        }
    }
    let plugins = project.as_ref().and_then(|project| plugin::load(project, diagnostics));
    if plugins.as_ref().is_some_and(|plugins| plugins.plugins.iter().any(|plugin| plugin.instance))
        && let Err(message) = write("deck.schema.json", &deck_schema(plugins.as_ref()))
    {
        diagnostics.push(Diagnostic::error("schema::write", message));
        return;
    }
    messages.push(editor_settings(project.as_ref()));
}

fn deck_schema(plugins: Option<&Plugins>) -> Value {
    let mut deck = config::schema_for::<DeckFile>();
    let instances: Vec<(&str, &ShortSchema)> = plugins
        .iter()
        .flat_map(|plugins| &plugins.plugins)
        .filter(|plugin| plugin.instance)
        .map(|plugin| (plugin.name.as_str(), &plugin.variables))
        .collect();
    if instances.is_empty() {
        return deck;
    }
    let mut variables = schema::variables_schema(&instances);
    if let (Some(Value::Object(defs)), Some(Value::Object(extra))) =
        (deck.get_mut("$defs"), variables.as_object_mut().and_then(|map| map.remove("$defs")))
    {
        defs.extend(extra);
    }
    deck["allOf"] = serde_json::json!([{ "properties": variables["properties"].take() }]);
    deck
}

fn editor_settings(project: Option<&Project>) -> String {
    let mut text = String::from("\nAdd to .taplo.toml next to deck.toml:\n\n");
    let mut notetypes: Vec<String> = Vec::new();
    let mut content: Vec<String> = Vec::new();
    if let Some(project) = project {
        notetypes = project.notetypes.values().map(|notetype| notetype.tree.source.name().to_string()).collect();
        collect_patterns(&project.deck.deck.deck, &mut content);
    }
    let toml: Vec<String> = content.iter().filter(|pattern| pattern.ends_with(".toml")).cloned().collect();
    for (include, schema) in [
        (vec!["deck.toml".to_string()], "deck.schema.json"),
        (notetypes, "notetype.schema.json"),
        (vec!["deck.lock".to_string()], "lock.schema.json"),
        (toml, "content.schema.json"),
    ] {
        if include.is_empty() {
            continue;
        }
        let _ = write!(
            text,
            "[[rule]]\ninclude = {}\n[rule.schema]\npath = \"{SCHEMA_DIR}/{schema}\"\n\n",
            Value::from(include)
        );
    }
    let yaml: Vec<&String> =
        content.iter().filter(|pattern| pattern.ends_with(".yaml") || pattern.ends_with(".yml")).collect();
    let json: Vec<&String> = content.iter().filter(|pattern| pattern.ends_with(".json")).collect();
    if !yaml.is_empty() || !json.is_empty() {
        let schema = format!("{SCHEMA_DIR}/content.schema.json");
        let settings = serde_json::json!({
            "yaml.schemas": { schema.clone(): yaml },
            "json.schemas": [{ "fileMatch": json, "url": format!("./{schema}") }],
        });
        let _ = write!(
            text,
            "Add to .vscode/settings.json for YAML and JSON content:\n\n{}\n",
            serde_json::to_string_pretty(&settings).unwrap_or_default()
        );
    }
    text
}

fn collect_patterns(deck: &Deck, out: &mut Vec<String>) {
    out.extend(deck.content.iter().cloned());
    for child in deck.children.values() {
        collect_patterns(child, out);
    }
}
