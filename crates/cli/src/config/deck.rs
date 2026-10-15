//! The serde types of `deck.toml`.
//!
//! [`DeckFile`] is the whole file. Its `[deck]` table is a [`RootDeck`]: the root [`Deck`] plus
//! the keys that only the project has (`output`, `media_dirs` and `new_order`). Child decks nest under
//! `children` and use [`Deck`] itself. `[presets.<key>]` tables are [`Preset`] values,
//! `[notetypes.<key>]` tables are [`NotetypeRef`] values and `[plugins.<name>]` tables are
//! [`PluginInstance`] values. A plugin instance can have an [`Http`] table with a [`RateLimit`]
//! (its unit is a [`RateUnit`]) and a [`Retry`] policy. The `[publish]` table is a [`Publish`]
//! value.
//!
//! The types that derive [`JsonSchema`], and their enum variants, have no doc comments on
//! purpose. schemars would copy a doc comment into the JSON Schema as a title or description.
//! A field has a doc comment only if its `#[schemars]` attribute sets both keys, because these
//! keys then win. The `title` and `description` attributes are the text that editors show.
//!
//! A [`NotetypeRef`] names a local note type with `path` alone, or a remote one with `git` or
//! `url`.
//!
//! A [`Preset`] can name a base preset in `extends`. [`DeckFile::preset_chain`] follows these
//! names. Every option of a [`Preset`] is optional. `None` sends nothing, and Anki keeps its own
//! default. The enum options come from [`janky_anki`].
//!
//! [`SecretSource`] and [`Alias`] implement [`JsonSchema`] by hand: the first needs `env` when
//! `file` is absent, and the second accepts a string or a list of strings.

use std::borrow::Cow;

use indexmap::IndexMap;
use janky_anki::{LeechAction, NewGatherOrder, NewSortOrder, ReviewMix, ReviewSortOrder};
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Deserializer};

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeckFile {
    #[schemars(
        title = r#"Root deck"#,
        description = r#"The root deck. It is also the project: its name is the project name."#
    )]
    pub deck: RootDeck,
    /// Keyed by the preset key, in file order.
    #[schemars(
        title = r#"Presets"#,
        description = r#"Anki deck options ("presets"). A deck uses a preset through its `preset` key. Anki adds a preset only on the first import."#
    )]
    #[serde(default)]
    pub presets: IndexMap<String, Preset>,
    /// Keyed by the note type key, in file order.
    #[schemars(
        title = r#"Note types"#,
        description = r#"The note types of the project. The key is the note type's identity and one of its entry types."#
    )]
    #[serde(default)]
    pub notetypes: IndexMap<String, NotetypeRef>,
    /// Keyed by the instance name, in file order.
    #[schemars(
        title = r#"Plugins"#,
        description = r#"Plugin instances: a Lua script and its settings. The key is the instance name. One script can have several instances."#
    )]
    #[serde(default)]
    pub plugins: IndexMap<String, PluginInstance>,
    /// `None` when the project does not publish.
    #[schemars(
        title = r#"Publish"#,
        description = r#"The Anki sync server and account that `janky publish` and `janky pull` use."#
    )]
    pub publish: Option<Publish>,
}

impl DeckFile {
    /// The preset `key`, then each preset that `extends` names in turn. The chain stops before
    /// an unknown key or a key that it already holds.
    pub fn preset_chain<'a>(&'a self, key: &'a str) -> Vec<&'a str> {
        let mut chain = Vec::new();
        let mut next = Some(key);
        while let Some(current) = next.filter(|current| self.presets.contains_key(*current) && !chain.contains(current))
        {
            chain.push(current);
            next = self.presets[current].extends.as_deref();
        }
        chain
    }

    /// The keys of the presets that a deck names in `preset`, in file order. Only these presets
    /// go into the package.
    pub fn used_presets(&self) -> Vec<&String> {
        let mut named = Vec::new();
        let mut decks = vec![&self.deck.deck];
        while let Some(deck) = decks.pop() {
            named.extend(deck.preset.as_ref());
            decks.extend(deck.children.values());
        }
        self.presets.keys().filter(|key| named.contains(key)).collect()
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PluginInstance {
    #[schemars(
        title = r#"Plugin script"#,
        description = r#"The Lua script of the plugin, relative to deck.toml. It must stay inside the project directory."#
    )]
    pub path: String,
    #[schemars(
        title = r#"Variables"#,
        description = r#"Settings that the plugin reads from `janky.variables`. The plugin's `variables` schema checks them."#
    )]
    #[serde(default)]
    pub variables: serde_json::Map<String, serde_json::Value>,
    /// `None` gives the plugin instance no network access.
    #[schemars(
        title = r#"Network access"#,
        description = r#"The hosts that the plugin's `generate` function may call, and the request limits. Absent: no network."#
    )]
    pub http: Option<Http>,
    /// Keyed by the secret name in the plugin.
    #[schemars(
        title = r#"Secrets"#,
        description = r#"Where each secret that the plugin uses comes from. The key is the secret name in the plugin. Absent: no secrets."#
    )]
    #[schemars(extend("propertyNames" = { "pattern": "^[A-Za-z_][A-Za-z0-9_]*$" }))]
    #[serde(default)]
    pub secrets: IndexMap<String, SecretSource>,
    /// `None` gives the plugin instance only its own `cache` area.
    #[schemars(
        title = r#"File access"#,
        description = r#"Read-only directories that the plugin's `generate` and `prepare` functions may read with `janky.fs`. Absent: only the instance's own `cache` area."#
    )]
    pub filesystem: Option<Filesystem>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Filesystem {
    /// Keyed by the name that the plugin uses as the first segment of a path.
    #[schemars(
        title = r#"Readable directories"#,
        description = r#"Directories that the plugin may read. The key is the name of the directory inside the plugin. The value is a path relative to deck.toml, or a path that starts with `~/` for the home directory. The name `cache` is taken: it is the instance's own file area."#
    )]
    #[schemars(extend("propertyNames" = { "pattern": "^[A-Za-z_][A-Za-z0-9_]*$", "not": { "const": "cache" } }))]
    #[serde(default)]
    pub read: IndexMap<String, String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Http {
    /// Each host matches [`HOST_PATTERN`].
    #[schemars(
        title = r#"Allowed hosts"#,
        description = r#"Host names that requests may go to: domain names, `localhost` or IPv4 addresses. `*.example.com` allows every subdomain of example.com. Default: none."#,
        inner(regex(pattern = HOST_PATTERN), length(max = 253))
    )]
    #[serde(default)]
    pub allowed_hosts: Vec<String>,
    #[schemars(
        title = r#"Maximum concurrency"#,
        description = r#"How many requests can be in flight at the same time. Default: 1."#,
        range(min = 1, max = 64)
    )]
    #[serde(default = "one")]
    pub max_concurrency: u32,
    /// `None` sets no limit.
    #[schemars(
        title = r#"Rate limit"#,
        description = r#"At most `count` requests per `unit`. Default: no limit."#
    )]
    pub rate_limit: Option<RateLimit>,
    #[schemars(
        title = r#"Retries"#,
        description = r#"Retries after 429, 5xx (not 501) and transport errors. Default: 4 attempts, 1000 ms base delay, 30000 ms maximum delay."#
    )]
    #[serde(default)]
    pub retry: Retry,
}

/// The pattern that each item of [`Http::allowed_hosts`] must match.
///
/// It accepts `localhost`, a domain name with an optional `*.` prefix, or an IPv4 address. A
/// schema error for this pattern gets its own message and help in the diagnostics.
pub const HOST_PATTERN: &str = r"^(localhost|(\*\.)?([a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?\.)+[a-z]([a-z0-9-]{0,61}[a-z0-9])?|((25[0-5]|2[0-4][0-9]|1[0-9][0-9]|[1-9]?[0-9])\.){3}(25[0-5]|2[0-4][0-9]|1[0-9][0-9]|[1-9]?[0-9]))$";

fn one() -> u32 {
    1
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RateLimit {
    #[schemars(
        title = r#"Count"#,
        description = r#"The number of requests per unit."#,
        range(min = 1)
    )]
    pub count: u32,
    #[schemars(title = r#"Unit"#, description = r#"`second`, `minute` or `hour`."#)]
    pub unit: RateUnit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum RateUnit {
    Second,
    Minute,
    Hour,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Retry {
    #[schemars(
        title = r#"Maximum attempts"#,
        description = r#"Attempts in total, the first one included. Default: 4."#,
        range(min = 1, max = 10)
    )]
    #[serde(default = "four")]
    pub max_attempts: u32,
    /// In milliseconds.
    #[schemars(
        title = r#"Base delay"#,
        description = r#"The first back-off delay in milliseconds. It doubles after each attempt, with jitter. Default: 1000."#,
        range(min = 10, max = 60000)
    )]
    #[serde(default = "base_delay")]
    pub base_delay_ms: u64,
    /// In milliseconds.
    #[schemars(
        title = r#"Maximum delay"#,
        description = r#"The longest delay in milliseconds, also for `Retry-After`. Default: 30000."#,
        range(min = 10, max = 60000)
    )]
    #[serde(default = "max_delay")]
    pub max_delay_ms: u64,
}

impl Default for Retry {
    fn default() -> Self {
        Self {
            max_attempts: four(),
            base_delay_ms: base_delay(),
            max_delay_ms: max_delay(),
        }
    }
}

fn four() -> u32 {
    4
}

fn base_delay() -> u64 {
    1000
}

fn max_delay() -> u64 {
    30000
}

/// Where one secret of a plugin instance comes from.
///
/// The schema needs `env` when `file` is absent.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecretSource {
    /// The environment variable that holds the secret. If it is not set or empty, janky looks
    /// for the same name in `file`, then in `.env` next to `deck.toml`.
    pub env: Option<String>,
    /// With `env`, a `.env` file to look in. Without `env`, a file whose whole content, without
    /// the surrounding whitespace, is the secret. The path is relative to `deck.toml`, and `~/`
    /// starts at the home directory.
    pub file: Option<String>,
}

impl JsonSchema for SecretSource {
    fn schema_name() -> Cow<'static, str> {
        "SecretSource".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "object",
            "properties": {
                "env": {
                    "title": "Environment variable",
                    "description": "The environment variable that holds the secret. If it is not set, janky looks for the same name in `file`, then in `.env` next to deck.toml.",
                    "type": "string",
                    "pattern": "^[A-Za-z_][A-Za-z0-9_]*$",
                },
                "file": {
                    "title": "File",
                    "description": "With `env`: a `.env` file to look in, relative to deck.toml. Alone: a file whose content is the secret, without surrounding whitespace. `~` is the home directory.",
                    "type": "string",
                    "minLength": 1,
                },
            },
            "if": { "not": { "required": ["file"] } },
            "then": { "required": ["env"] },
            "additionalProperties": false,
        })
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Publish {
    #[schemars(
        title = r#"Sync server"#,
        description = r#"The address of the Anki sync server, for example `https://anki.example.org/`. It must use `https://`, or `http://` on a loopback address. A path must end with `/`."#
    )]
    #[schemars(length(min = 1))]
    pub endpoint: String,
    #[schemars(
        title = r#"User name"#,
        description = r#"The user name of the sync account. On a self-hosted server, it is the name before the colon in `SYNC_USER1`."#
    )]
    #[schemars(length(min = 1))]
    pub username: String,
    #[schemars(
        title = r#"Password"#,
        description = r#"Where the password of the sync account comes from: an environment variable (`env`), a file (`file`), or both, as for plugin secrets."#
    )]
    pub password: SecretSource,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RootDeck {
    /// `None` uses `.janky/build/deck.apkg`.
    #[schemars(
        title = r#"Package path"#,
        description = r#"Where `janky build` writes the Anki package. Root deck only. It must stay inside the project directory. Default: `.janky/build/deck.apkg`."#
    )]
    pub output: Option<String>,
    /// Empty uses `media`.
    #[schemars(
        title = r#"Media directories"#,
        description = r#"The directories where `media()` looks for referenced files, in this order. Root deck only. Default: `["media"]`. A directory that does not exist is a warning."#
    )]
    #[serde(default)]
    pub media_dirs: Vec<String>,
    #[schemars(
        title = r#"New card order"#,
        description = r#"The order of new cards inside one content file. Root deck only. The files stay in deck order, then pattern order, then path order. `file`: the entry order in the file. `shuffle-in-file`: a fixed order from a hash of each note's GUID. Anki keeps the position of cards that it already has. Default: `file`."#
    )]
    #[serde(default)]
    pub new_order: NewOrder,
    #[serde(flatten)]
    pub deck: Deck,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum NewOrder {
    #[default]
    File,
    ShuffleInFile,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Deck {
    #[schemars(
        title = r#"Deck name"#,
        description = r#"The deck name that Anki shows. The full Anki name is the parent's full name, `::` and this name. A changed name makes a new deck when you import."#
    )]
    pub name: String,
    /// A key of [`DeckFile::presets`]. `None` uses Anki's default preset.
    #[schemars(
        title = r#"Preset"#,
        description = r#"The key of a `[presets.<key>]` table. Default: Anki's "Default" preset."#
    )]
    pub preset: Option<String>,
    /// Glob patterns relative to the project directory.
    #[schemars(
        title = r#"Content files"#,
        description = r#"Glob patterns for the content files of this deck (`.yaml`, `.yml`, `.toml`, `.json`). A file that two decks match is an error. Default: none."#
    )]
    #[serde(default)]
    pub content: Vec<String>,
    /// Keyed by the deck key, in file order.
    #[schemars(
        title = r#"Child decks"#,
        description = r#"Child decks. The key is the deck's identity in the project."#
    )]
    #[serde(default)]
    pub children: IndexMap<String, Deck>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Preset {
    /// `None` uses the preset key.
    #[schemars(
        title = r#"Preset name"#,
        description = r#"The preset name that Anki shows. Default: the preset key."#
    )]
    pub name: Option<String>,
    /// A key of [`DeckFile::presets`].
    #[schemars(
        title = r#"Extends"#,
        description = r#"The key of another preset. This preset gets each option of that preset that it does not set itself, except `name`."#
    )]
    pub extends: Option<String>,
    #[schemars(title = r#"New cards per day"#, description = r#"0 to 9999. Anki default: 20."#)]
    #[schemars(range(max = 9999))]
    pub new_per_day: Option<u32>,
    #[schemars(title = r#"Reviews per day"#, description = r#"0 to 9999. Anki default: 200."#)]
    #[schemars(range(max = 9999))]
    pub reviews_per_day: Option<u32>,
    #[schemars(
        title = r#"Learning steps"#,
        description = r#"Steps for new cards: a number and `s`, `m`, `h` or `d`, for example `10m`. Anki default: `["1m", "10m"]`."#
    )]
    #[schemars(inner(pattern(r"^[0-9]+(\.[0-9]+)?[smhd]$")))]
    pub learning_steps: Option<Vec<String>>,
    #[schemars(
        title = r#"Relearning steps"#,
        description = r#"Steps for forgotten cards: a number and `s`, `m`, `h` or `d`. Anki default: `["10m"]`."#
    )]
    #[schemars(inner(pattern(r"^[0-9]+(\.[0-9]+)?[smhd]$")))]
    pub relearning_steps: Option<Vec<String>>,
    #[schemars(
        title = r#"Desired retention"#,
        description = r#"The probability of recall that FSRS aims for, 0.7 to 0.99. Anki default: 0.9."#
    )]
    #[schemars(range(min = 0.7, max = 0.99))]
    pub desired_retention: Option<f64>,
    #[schemars(
        title = r#"Maximum interval"#,
        description = r#"The longest interval: a whole number and `d`, `w` or `y` (365 days), at most 36500 days. Anki default: `36500d`."#
    )]
    #[schemars(pattern(r"^[1-9][0-9]*[dwy]$"))]
    pub maximum_interval: Option<String>,
    /// The schema accepts any length. The package step reports a length other than 17, 19
    /// or 21.
    #[schemars(
        title = r#"FSRS parameters"#,
        description = r#"17, 19 or 21 numbers. Normally leave this out: without it, janky sends nothing and Anki uses its built-in values."#
    )]
    pub fsrs_params: Option<Vec<f64>>,
    #[schemars(
        title = r#"New card gather order"#,
        description = r#"How Anki gathers new cards from the deck tree. Anki default: `deck`."#
    )]
    pub new_gather_order: Option<NewGatherOrder>,
    #[schemars(
        title = r#"New card sort order"#,
        description = r#"How Anki sorts the gathered new cards. Anki default: `template`."#
    )]
    pub new_sort_order: Option<NewSortOrder>,
    #[schemars(
        title = r#"Review sort order"#,
        description = r#"How Anki sorts review cards. Anki default: `due`."#
    )]
    pub review_sort_order: Option<ReviewSortOrder>,
    #[schemars(
        title = r#"New and review order"#,
        description = r#"When Anki shows new cards, compared with reviews. Anki default: `mix-with-reviews`."#
    )]
    pub new_review_mix: Option<ReviewMix>,
    #[schemars(
        title = r#"Interday learning and review order"#,
        description = r#"When Anki shows learning cards with steps of a day or more. Anki default: `mix-with-reviews`."#
    )]
    pub interday_learning_mix: Option<ReviewMix>,
    #[schemars(
        title = r#"Bury new siblings"#,
        description = r#"Hide the other new cards of a note until the next day. Anki default: false."#
    )]
    pub bury_new: Option<bool>,
    #[schemars(
        title = r#"Bury review siblings"#,
        description = r#"Hide the other review cards of a note until the next day. Anki default: false."#
    )]
    pub bury_reviews: Option<bool>,
    #[schemars(
        title = r#"Bury interday learning siblings"#,
        description = r#"Hide the other interday learning cards of a note until the next day. Anki default: false."#
    )]
    pub bury_interday_learning: Option<bool>,
    #[schemars(
        title = r#"Leech threshold"#,
        description = r#"The number of lapses after which a card is a leech, 1 to 9999. Anki default: 8."#
    )]
    #[schemars(range(min = 1, max = 9999))]
    pub leech_threshold: Option<u32>,
    #[schemars(
        title = r#"Leech action"#,
        description = r#"`tag-only` adds the `leech` tag. `suspend` also suspends the card. Anki default: `tag-only`."#
    )]
    pub leech_action: Option<LeechAction>,
    #[schemars(title = r#"Play audio automatically"#, description = r#"Anki default: true."#)]
    pub autoplay: Option<bool>,
    #[schemars(
        title = r#"Replay question audio"#,
        description = r#"Play the question audio again when you replay the answer. Anki default: true."#
    )]
    pub replay_question_audio: Option<bool>,
    #[schemars(title = r#"Show answer timer"#, description = r#"Anki default: false."#)]
    pub show_timer: Option<bool>,
    #[schemars(
        title = r#"Maximum answer seconds"#,
        description = r#"The longest answer time that Anki records, 1 to 9999. Anki default: 60."#
    )]
    #[schemars(range(min = 1, max = 9999))]
    pub max_answer_seconds: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NotetypeRef {
    #[schemars(
        title = r#"Note type file"#,
        description = r#"A note type file, or a directory with `notetype.toml`. Local: relative to deck.toml, required. Remote: inside the source, optional."#
    )]
    pub path: Option<String>,
    #[schemars(
        title = r#"Git repository"#,
        description = r#"A public Git repository over `https://` or `git://`. Needs one of `tag`, `rev` or `branch`."#
    )]
    pub git: Option<String>,
    #[schemars(
        title = r#"Git tag"#,
        description = r#"The tag to build from. deck.lock records its commit."#
    )]
    pub tag: Option<String>,
    #[schemars(
        title = r#"Git commit"#,
        description = r#"The commit to build from (a full or short id)."#
    )]
    pub rev: Option<String>,
    #[schemars(
        title = r#"Git branch"#,
        description = r#"The branch to build from. deck.lock records its commit until `janky update`."#
    )]
    pub branch: Option<String>,
    #[schemars(
        title = r#"Archive URL"#,
        description = r#"A `.tar.gz`, `.tgz` or `.zip` archive over `https://`. Needs `sha256`."#
    )]
    pub url: Option<String>,
    #[schemars(
        title = r#"Archive SHA-256"#,
        description = r#"The SHA-256 hash of the archive: 64 hex digits."#
    )]
    #[schemars(pattern(r"^[0-9a-f]{64}$"))]
    pub sha256: Option<String>,
    #[schemars(
        title = r#"Entry type aliases"#,
        description = r#"More entry types for this note type: a string or a list of strings. The key is always an entry type. An entry type can belong to one note type only."#
    )]
    #[serde(default)]
    pub alias: Alias,
    #[schemars(
        title = r#"Default note type"#,
        description = r#"Use this note type for entries without `type`. At most one note type can be the default. Default: false."#
    )]
    #[serde(default)]
    pub default: bool,
}

impl NotetypeRef {
    /// The entry types of the note type with this `key`: the key first, then each alias in
    /// order, without duplicates.
    pub fn entry_types(&self, key: &str) -> Vec<String> {
        let mut names = vec![key.to_string()];
        for name in &self.alias.names {
            if !names.contains(name) {
                names.push(name.clone());
            }
        }
        names
    }
}

/// The `alias` value of a note type: one entry type name or a list of them.
#[derive(Debug, Default)]
pub struct Alias {
    /// The entry type names, in file order. A single string gives one name.
    pub names: Vec<String>,
    /// Whether the file has a list. Diagnostics use it to point at the right list item.
    pub is_list: bool,
}

impl<'de> Deserialize<'de> for Alias {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            One(String),
            Many(Vec<String>),
        }
        Ok(match Raw::deserialize(deserializer)? {
            Raw::One(name) => Self { names: vec![name], is_list: false },
            Raw::Many(names) => Self { names, is_list: true },
        })
    }
}

impl JsonSchema for Alias {
    fn schema_name() -> Cow<'static, str> {
        "Alias".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({ "if": { "type": "string" }, "else": { "type": "array", "items": { "type": "string" } } })
    }
}
