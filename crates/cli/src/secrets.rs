//! Reads the secrets that generator plugins use, such as API keys.
//!
//! A plugin lists the secret names it uses. `[plugins.<name>.secrets]` in `deck.toml` gives a
//! source for each one: an environment variable (`env`), a file (`file`), or both. [`resolve`]
//! reads every secret before any request goes out, and reports each one that is missing or not
//! valid.
//!
//! Each value is registered for redaction as soon as it is read, so logs and errors show
//! `<secret:name>` in its place. [`Secrets`] does not show values in its `Debug` output. A
//! value must be printable ASCII without line breaks, so that it can be a header value.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::config::deck::SecretSource;
use crate::diagnostic::{Diagnostic, Diagnostics};
use crate::plugin::Plugins;
use crate::project::Project;
use crate::tree::child;

const FILE_LIMIT: u64 = 64 * 1024;
/// The shortest secret value that janky accepts, in characters.
pub const MIN_LENGTH: usize = 8;
/// The longest secret value that janky accepts, in characters.
pub const MAX_LENGTH: usize = 8 * 1024;

/// The secret values of each plugin instance, keyed by instance name and then secret name.
///
/// Its `Debug` output shows only the names.
#[derive(Default, Clone)]
pub struct Secrets {
    values: HashMap<String, HashMap<String, String>>,
}

impl std::fmt::Debug for Secrets {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let names: Vec<String> = self
            .values
            .iter()
            .flat_map(|(instance, names)| names.keys().map(move |name| format!("{instance}.{name}")))
            .collect();
        f.debug_struct("Secrets").field("names", &names).finish()
    }
}

impl Secrets {
    /// Returns the value of a secret of an instance, or `None` when it was not read.
    pub fn get(&self, instance: &str, name: &str) -> Option<&str> {
        self.values.get(instance).and_then(|names| names.get(name)).map(String::as_str)
    }

    /// Returns the name and value of each secret of an instance.
    pub fn values(&self, instance: &str) -> impl Iterator<Item = (&String, &String)> {
        self.values.get(instance).into_iter().flatten()
    }

    fn insert(&mut self, instance: &str, name: &str, value: String) {
        crate::diagnostic::redact(&value, name);
        self.values.entry(instance.to_string()).or_default().insert(name.to_string(), value);
    }
}

/// Reads the secrets of every plugin instance that has a `generate` function.
///
/// It reads only the secrets that the plugin lists and that have a source in `deck.toml`. For a
/// source with `env`, the order is the environment variable, then the variable in the `file` of
/// the source (as a `.env` file), then the variable in `.env` next to `deck.toml`. A source
/// with only `file` uses the whole file content, trimmed. A `file` path that starts with `~/`
/// is relative to the home directory, and other paths are relative to the project root.
///
/// It pushes a `secret::missing` error at the source in `deck.toml` for each secret that it
/// cannot read or that is not valid. For a missing variable, the message names the places
/// where janky looked.
///
/// It also registers the password of `[publish]` for redaction, without an error when it is
/// missing. Plugins then cannot read it through `janky.fs` or send it through `janky.http`.
pub fn resolve(project: &Project, plugins: &Plugins, diagnostics: &mut Diagnostics) -> Secrets {
    if let Some(publish) = &project.deck.publish
        && let Ok(value) = read(&project.root, &publish.password)
    {
        crate::diagnostic::redact(&value, "password");
    }
    let mut secrets = Secrets::default();
    let mut found = Diagnostics::default();
    for plugin in plugins.plugins.iter().filter(|plugin| plugin.instance && plugin.generate.is_some()) {
        let Some(instance) = project.deck.plugins.get(&plugin.name) else { continue };
        for name in &plugin.secrets {
            let Some(source) = instance.secrets.get(name) else { continue };
            let pointer = child(&child(&child("/plugins", &plugin.name), "secrets"), name);
            match read(&project.root, source) {
                Ok(value) => secrets.insert(&plugin.name, name, value),
                Err(problem) => {
                    let (loc, _) = project.deck_tree.nearest(&pointer);
                    found.push(
                        Diagnostic::error(
                            "secret::missing",
                            format!("the secret `{name}` of `{}`: {problem}", plugin.name),
                        )
                        .with_source(project.deck_tree.source.clone())
                        .with_label(loc.value.clone(), "here"),
                    );
                },
            }
        }
    }
    diagnostics.append_sorted(found);
    secrets
}

/// Reads the password of `[publish]` in the same way as a plugin secret, and registers it for
/// redaction. `None` when the project has no `[publish]` table, or when the password cannot be
/// read or is not valid. That gives a `secret::missing` error at its source in `deck.toml`.
pub fn publish_password(project: &Project, diagnostics: &mut Diagnostics) -> Option<String> {
    let publish = project.deck.publish.as_ref()?;
    match read(&project.root, &publish.password) {
        Ok(value) => {
            crate::diagnostic::redact(&value, "password");
            Some(value)
        },
        Err(problem) => {
            let (loc, _) = project.deck_tree.nearest("/publish/password");
            diagnostics.push(
                Diagnostic::error("secret::missing", format!("the password of `[publish]`: {problem}"))
                    .with_source(project.deck_tree.source.clone())
                    .with_label(loc.value.clone(), "here"),
            );
            None
        },
    }
}

fn read(root: &Path, source: &SecretSource) -> Result<String, String> {
    let value = match (&source.env, &source.file) {
        (Some(variable), file) => {
            let from_env =
                std::env::var(variable).ok().map(|value| value.trim().to_string()).filter(|value| !value.is_empty());
            match from_env {
                Some(value) => value,
                None => {
                    let mut files = file.iter().map(|file| path(root, file)).collect::<Result<Vec<_>, _>>()?;
                    files.push(root.join(".env"));
                    files
                        .iter()
                        .find_map(|file| {
                            dotenv(file)
                                .and_then(|pairs| pairs.get(variable).cloned())
                                .filter(|value| !value.is_empty())
                        })
                        .ok_or_else(|| {
                            let places: Vec<String> =
                                files.iter().map(|file| format!("`{}`", file.display())).collect();
                            format!("set the environment variable `{variable}`, or add it to {}", places.join(" or "))
                        })?
                },
            }
        },
        (None, Some(file)) => {
            let file = path(root, file)?;
            let text = read_limited(&file).map_err(|error| format!("cannot read `{}`: {error}", file.display()))?;
            text.trim().to_string()
        },
        (None, None) => return Err("the source has neither `env` nor `file`".into()),
    };
    if value.chars().any(char::is_control) {
        return Err("the value has a line break or another control character".into());
    }
    if !value.chars().all(|c| c.is_ascii_graphic() || c == ' ') {
        return Err("the value must be printable ASCII, so that it can be a header value".into());
    }
    if value.len() < MIN_LENGTH || value.len() > MAX_LENGTH {
        return Err(format!("the value must have {MIN_LENGTH} to {MAX_LENGTH} characters"));
    }
    Ok(value)
}

/// Resolves a configured path: `~/…` against the home directory, an error when `HOME` is not
/// set or is empty, and any other path against `root`.
pub(crate) fn path(root: &Path, file: &str) -> Result<PathBuf, String> {
    match file.strip_prefix("~/") {
        Some(rest) => std::env::var_os("HOME")
            .filter(|home| !home.is_empty())
            .map(|home| PathBuf::from(home).join(rest))
            .ok_or_else(|| format!("`HOME` is not set, so `{file}` cannot be found")),
        None => Ok(root.join(file)),
    }
}

fn read_limited(path: &Path) -> std::io::Result<String> {
    if !std::fs::metadata(path)?.is_file() {
        return Err(std::io::Error::other("not a regular file"));
    }
    let mut text = String::new();
    std::fs::File::open(path)?.take(FILE_LIMIT).read_to_string(&mut text)?;
    Ok(text)
}

/// Reads a `.env` file of at most 64 KiB and parses it with [`parse_dotenv`]. `None` when
/// the path is not a regular file or cannot be read.
pub fn dotenv(path: &Path) -> Option<HashMap<String, String>> {
    let text = read_limited(path).ok()?;
    Some(parse_dotenv(&text))
}

/// Parses the text of a `.env` file into variable names and values.
///
/// It skips empty lines, comment lines and lines without `=`, and removes an `export ` prefix
/// and a byte order mark. A value in single or double quotes ends at the next quote on the same
/// line, and a line with an unclosed quote is skipped. An unquoted value ends before ` #`. A
/// later line wins over an earlier line with the same name.
pub fn parse_dotenv(text: &str) -> HashMap<String, String> {
    let mut pairs = HashMap::new();
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line);
        let Some((name, value)) = line.split_once('=') else { continue };
        let value = value.trim();
        let value = match value.chars().next() {
            Some(quote @ ('"' | '\'')) => match value[1..].split_once(quote) {
                Some((inner, _)) => inner,
                None => continue,
            },
            _ => value.split(" #").next().unwrap_or_default().trim(),
        };
        pairs.insert(name.trim().to_string(), value.to_string());
    }
    pairs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(env: Option<&str>, file: Option<&str>) -> SecretSource {
        SecretSource {
            env: env.map(String::from),
            file: file.map(String::from),
        }
    }

    #[test]
    fn dotenv_files_are_parsed() {
        let pairs = parse_dotenv(
            "\u{feff}# comment\nPLAIN=abc123\nexport QUOTED=\"with space\" \nSINGLE='x=y'\nTRAILING=value # note\n\nbroken\nOPEN=\"first\nline\"\n",
        );
        assert_eq!(pairs["PLAIN"], "abc123");
        assert_eq!(pairs["QUOTED"], "with space");
        assert_eq!(pairs["SINGLE"], "x=y");
        assert_eq!(pairs["TRAILING"], "value");
        assert!(!pairs.contains_key("broken"));
        assert!(!pairs.contains_key("OPEN"));
    }

    #[test]
    fn sources_are_read_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join(".env"), "JANKY_TEST_ONE=from-dot-env\n").unwrap();
        std::fs::write(root.join(".env.local"), "JANKY_TEST_ONE=from-local-file\n").unwrap();
        std::fs::write(root.join("key.txt"), "  file-content-key\n").unwrap();
        assert_eq!(read(root, &source(Some("JANKY_TEST_ONE"), None)).unwrap(), "from-dot-env");
        assert_eq!(read(root, &source(Some("JANKY_TEST_ONE"), Some(".env.local"))).unwrap(), "from-local-file");
        assert_eq!(read(root, &source(None, Some("key.txt"))).unwrap(), "file-content-key");
        let missing = read(root, &source(Some("JANKY_TEST_MISSING"), None)).unwrap_err();
        assert!(missing.contains("set the environment variable `JANKY_TEST_MISSING`"), "{missing}");
        std::fs::write(root.join("short.txt"), "abc").unwrap();
        assert!(read(root, &source(None, Some("short.txt"))).unwrap_err().contains("8 to"));
        std::fs::write(root.join("broken.txt"), "first-line\nsecond-line").unwrap();
        assert!(read(root, &source(None, Some("broken.txt"))).unwrap_err().contains("line break"));
    }
}
