//! Jobs for generated media and data, and the checks and hashes that name their cached results.
//!
//! The `media(input, plugin)` and `enrich(input, plugin)` template functions record one [`Job`]
//! for each kind, plugin instance and input. A plugin's optional `plan` function gives the cache
//! key, the file extension and the billable units, and [`parse_plan`] checks its result.
//! [`job_hash`] turns the plugin id, the cache key and the input into the hash that names the
//! result in the media cache or the data cache (see [`crate::cache`]). Recording a job sends
//! nothing. `janky generate` runs the jobs that [`pending`] returns.
//!
//! An input is text or a map ([`job_input`]). [`job_hash`] gives a map input or a data job its
//! own version tag, so a text input for media keeps the hash that it had before maps existed.

use std::sync::{Arc, Mutex};

use indexmap::IndexMap;
use serde::Serialize;
use serde_json::Value as Json;
use unicode_normalization::UnicodeNormalization;

/// What a job makes: a media file, or data for `enrich`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// A file in the media cache, from `media`.
    Media,
    /// A JSON object in the data cache, from `enrich`.
    Data,
}

impl Kind {
    /// The name that `plan` and `generate` see in `job.kind`.
    pub fn name(self) -> &'static str {
        match self {
            Kind::Media => "media",
            Kind::Data => "data",
        }
    }
}

/// One request to a generator for one input, recorded while the fields render.
///
/// Two calls with the same plugin instance and the same input share one job.
#[derive(Debug, Clone, Serialize)]
pub struct Job {
    /// The name of the plugin instance in `[plugins]`.
    pub instance: String,
    /// What the job makes. It is the kind that the plugin declares in `generates`.
    pub kind: Kind,
    /// The input from [`job_input`]: NFC-normalised text, or a map in [`canonical`] form.
    pub input: Json,
    /// The [`job_hash`] of the job. Jobs with the same hash give the same file.
    pub hash: String,
    /// The cache key in [`canonical`] form: the key from `plan`, or else the instance variables.
    pub key: Json,
    /// The file extension from `plan`. `None` when the plugin has no `plan`, when `plan` gives
    /// none, or for a data job.
    pub ext: Option<String>,
    /// The billable units from `plan`, counted in [`unit`](Self::unit).
    pub units: Option<u64>,
    /// The name of the billable unit from `plan`, such as `characters`.
    pub unit: Option<String>,
    /// The media file name, `<hash>.<ext>`. It is empty when the result is not cached and
    /// `plan` gives no extension, because the name is not known before the first run. It is
    /// always empty for a data job.
    pub file: String,
    /// Whether the result is in the media cache, or in the data cache for a data job.
    pub exists: bool,
}

impl Job {
    /// The input as one line of text: the text itself, or the map as canonical JSON.
    pub fn input_text(&self) -> String {
        match &self.input {
            Json::String(text) => text.clone(),
            other => other.to_string(),
        }
    }
}

/// The largest cache key that `plan` can return, in bytes of canonical JSON.
pub const KEY_LIMIT: usize = 64 * 1024;
/// The largest map input, in bytes of canonical JSON.
pub const INPUT_LIMIT: usize = 64 * 1024;

/// The jobs of one render, shared by the template functions, in the order they were recorded.
///
/// The map key is the kind, the plugin instance and the input as JSON text, so a text input
/// and a map input never share a key.
pub type Jobs = Arc<Mutex<IndexMap<(Kind, String, String), Job>>>;

/// Converts the input of `generate` or `enrich` to the input of a job.
///
/// A map that can be iterated becomes a JSON object in [`canonical`] form. Its text is
/// NFC-normalised, also in keys. A key with an undefined or `none` value is left out, because a
/// plugin cannot tell it from a missing key. An integer stays exact, and a whole float within
/// the range of an integer becomes an integer, so `1.0` and `1` give the same job. A map can hold
/// text, numbers, `true` and `false`, lists and maps, at most [`crate::host::DEPTH_LIMIT`] levels
/// deep and [`INPUT_LIMIT`] bytes as JSON. Two keys that are the same after normalisation are
/// an error. Any other value, a media value too, becomes its text, NFC-normalised, as before
/// maps existed.
///
/// Gives back `None` when the input is empty: text that is empty or only white space, or a map
/// with no keys left. Such an input records no job. The error is a message for the author.
pub fn job_input(value: &minijinja::Value) -> Result<Option<Json>, String> {
    let media = value.downcast_object_ref::<crate::render::Media>().is_some();
    if value.kind() != minijinja::value::ValueKind::Map || media || value.try_iter().is_err() {
        let text: String = value.to_string().nfc().collect();
        return Ok((!text.trim().is_empty()).then_some(Json::String(text)));
    }
    let mut budget = INPUT_LIMIT;
    let json = template_json(value, 0, &mut budget)?.unwrap_or_default();
    let json = canonical(&json);
    if json.as_object().is_some_and(serde_json::Map::is_empty) {
        return Ok(None);
    }
    if json.to_string().len() > INPUT_LIMIT {
        return Err(format!("the input is longer than {INPUT_LIMIT} bytes as JSON"));
    }
    Ok(Some(json))
}

fn template_json(value: &minijinja::Value, depth: usize, budget: &mut usize) -> Result<Option<Json>, String> {
    use minijinja::value::ValueKind;
    if depth > crate::host::DEPTH_LIMIT {
        return Err(format!("the input is nested more than {} levels deep", crate::host::DEPTH_LIMIT));
    }
    Ok(Some(match value.kind() {
        ValueKind::Undefined => return Ok(None),
        ValueKind::None => {
            spend(budget, 4)?;
            Json::Null
        },
        ValueKind::Bool => {
            spend(budget, 5)?;
            Json::Bool(value.is_true())
        },
        ValueKind::Number => {
            spend(budget, 20)?;
            number(value)?
        },
        ValueKind::String => {
            let text: String = value.to_string().nfc().collect();
            spend(budget, text.len() + 2)?;
            Json::String(text)
        },
        ValueKind::Seq => {
            let mut items = Vec::new();
            for item in value.try_iter().map_err(|error| error.to_string())? {
                spend(budget, 1)?;
                items.push(template_json(&item, depth + 1, budget)?.unwrap_or(Json::Null));
            }
            Json::Array(items)
        },
        ValueKind::Map => {
            let mut object = serde_json::Map::new();
            for key in value.try_iter().map_err(|error| error.to_string())? {
                let item = value.get_item(&key).map_err(|error| error.to_string())?;
                if item.is_undefined() || item.is_none() {
                    continue;
                }
                let name: String = key.to_string().nfc().collect();
                spend(budget, name.len() + 4)?;
                let Some(item) = template_json(&item, depth + 1, budget)? else { continue };
                if object.insert(name.clone(), item).is_some() {
                    return Err(format!("the input map has the key `{name}` twice, after NFC normalisation"));
                }
            }
            Json::Object(object)
        },
        _ => {
            return Err(format!(
                "an input map can hold only text, numbers, true and false, lists and maps, not {}",
                value.kind()
            ));
        },
    }))
}

fn spend(budget: &mut usize, bytes: usize) -> Result<(), String> {
    *budget =
        budget.checked_sub(bytes).ok_or_else(|| format!("the input is longer than {INPUT_LIMIT} bytes as JSON"))?;
    Ok(())
}

fn number(value: &minijinja::Value) -> Result<Json, String> {
    if let Ok(integer) = i64::try_from(value.clone()) {
        return Ok(Json::from(integer));
    }
    if let Ok(integer) = u64::try_from(value.clone()) {
        return Ok(Json::from(integer));
    }
    let float = f64::try_from(value.clone()).map_err(|error| error.to_string())?;
    if !float.is_finite() {
        return Err("an input number must be finite".into());
    }
    if float.fract() != 0.0 {
        return Ok(Json::from(float));
    }
    if (i64::MIN as f64..-(i64::MIN as f64)).contains(&float) {
        return Ok(Json::from(float as i64));
    }
    Err(format!("the input number {float} is too large to be exact"))
}

/// The checked result of a plugin's `plan` function. Every field is optional.
#[derive(Debug, Default, PartialEq)]
pub struct Planned {
    /// The file extension, checked by [`valid_ext`].
    pub ext: Option<String>,
    /// The cache key, as `plan` gave it. The caller makes it [`canonical`].
    pub key: Option<Json>,
    /// The billable units.
    pub units: Option<u64>,
    /// The name of the billable unit: 1 to 32 bytes with no control characters.
    pub unit: Option<String>,
}

/// Returns the jobs whose results are not in the cache, one for each hash, in their first order.
pub fn pending(jobs: &[Job]) -> Vec<&Job> {
    let mut seen = std::collections::HashSet::new();
    jobs.iter().filter(|job| !job.exists && seen.insert(job.hash.as_str())).collect()
}

/// Checks the result of `plan`, converted to JSON, and returns its parts.
///
/// An empty array counts as an empty table. A float `units` value is accepted if it is a whole
/// number below 10^15. The error is a message for the author. It comes back when the value is
/// not a table, a key is unknown, or a known key has a wrong value. A `key` is wrong when its
/// canonical JSON is longer than [`KEY_LIMIT`] bytes or holds U+FFFD.
pub fn parse_plan(value: Json) -> Result<Planned, String> {
    let object = match value {
        Json::Object(object) => object,
        Json::Array(items) if items.is_empty() => serde_json::Map::new(),
        _ => return Err("`plan` must return a table".into()),
    };
    let mut planned = Planned::default();
    for (name, value) in object {
        match (name.as_str(), value) {
            ("ext", Json::String(ext)) if valid_ext(&ext) => planned.ext = Some(ext),
            ("ext", _) => {
                return Err("`ext` must be 1 to 10 lower-case letters or digits, such as `ogg`, and not `json`".into());
            },
            ("key", key) => {
                let text = canonical(&key).to_string();
                if text.len() > KEY_LIMIT {
                    return Err(format!("`key` is longer than {KEY_LIMIT} bytes as JSON"));
                }
                if text.contains('\u{fffd}') {
                    return Err("`key` must be valid UTF-8 text".into());
                }
                planned.key = Some(key);
            },
            ("units", Json::Number(units)) => match units.as_u64() {
                Some(units) => planned.units = Some(units),
                None => match units.as_f64() {
                    Some(value) if value >= 0.0 && value.fract() == 0.0 && value < 1e15 => {
                        planned.units = Some(value as u64)
                    },
                    _ => return Err("`units` must be a whole number, 0 or more".into()),
                },
            },
            ("units", _) => return Err("`units` must be a whole number, 0 or more".into()),
            ("unit", Json::String(unit)) if valid_unit(&unit) => planned.unit = Some(unit),
            ("unit", _) => return Err("`unit` must be a short text, such as `characters`".into()),
            (other, _) => return Err(format!("unknown key `{other}` in the result of `plan`")),
        }
    }
    Ok(planned)
}

fn valid_unit(unit: &str) -> bool {
    !unit.is_empty() && unit.len() <= 32 && !unit.chars().any(|c| c.is_control() || c == '\u{fffd}')
}

/// Whether `ext` is 1 to 10 lower-case ASCII letters or digits, so it is safe in a file name,
/// and not `json`, which would write over the result's own `<hash>.json` metadata.
pub fn valid_ext(ext: &str) -> bool {
    (1..=10).contains(&ext.len())
        && ext.bytes().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        && ext != "json"
}

/// Returns a copy of `value` with the keys of every object sorted, so that equal values give
/// equal JSON text.
pub fn canonical(value: &Json) -> Json {
    match value {
        Json::Object(object) => {
            let mut keys: Vec<&String> = object.keys().collect();
            keys.sort();
            Json::Object(keys.into_iter().map(|key| (key.clone(), canonical(&object[key]))).collect())
        },
        Json::Array(items) => Json::Array(items.iter().map(canonical).collect()),
        other => other.clone(),
    }
}

/// Returns the cache hash of a media job with a text input: the first 32 hex digits (128 bits)
/// of BLAKE3 over the version tag `janky-media-1`, the plugin id, the key and the input, in
/// this order.
///
/// The key is made [`canonical`] first, so key order does not change the hash. Each part is
/// prefixed with its length as a little-endian `u64`, so that different splits of the same
/// bytes give different hashes. The input must already be NFC-normalised. [`job_hash`] calls
/// this.
pub fn media_hash(id: &str, key: &Json, input: &str) -> String {
    hash(&[
        b"janky-media-1",
        id.as_bytes(),
        canonical(key).to_string().as_bytes(),
        input.as_bytes(),
    ])
}

/// Returns the cache hash of a job of `kind` with `input`.
///
/// A text input of a media job gives its [`media_hash`], so its file keeps its name. Any other
/// job hashes the version tag `janky-job-2`, the kind name, the plugin id, the key and the input,
/// in this order, with the same length prefixes and BLAKE3. The key and the input are
/// [`canonical`] JSON. So a map input never shares a hash with a text input, and a data job
/// never shares a hash with a media job.
pub fn job_hash(id: &str, kind: Kind, key: &Json, input: &Json) -> String {
    match (kind, input) {
        (Kind::Media, Json::String(text)) => media_hash(id, key, text),
        _ => hash(&[
            b"janky-job-2",
            kind.name().as_bytes(),
            id.as_bytes(),
            canonical(key).to_string().as_bytes(),
            canonical(input).to_string().as_bytes(),
        ]),
    }
}

fn hash(parts: &[&[u8]]) -> String {
    let mut hasher = blake3::Hasher::new();
    for part in parts {
        hasher.update(&(part.len() as u64).to_le_bytes());
        hasher.update(part);
    }
    hasher.finalize().to_hex()[..32].to_string()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn input(expression: &str) -> Result<Option<Json>, String> {
        let env = minijinja::Environment::new();
        let entry = json!({ "name": "Ne\u{301}on", "level": 2.0 });
        let value = env
            .compile_expression(expression)
            .unwrap()
            .eval(minijinja::context! { entry => minijinja::Value::from(minijinja::value::Serde(&entry)) })
            .unwrap();
        job_input(&value)
    }

    #[test]
    fn map_inputs_are_canonical() {
        assert_eq!(
            input(
                "{'word': entry.name, 'gone': entry.missing, 'none': none, 'n': entry.level, 'f': 1.5, 'list': [entry.missing, none, 'a']}"
            ),
            Ok(Some(json!({ "f": 1.5, "list": [null, null, "a"], "n": 2, "word": "N\u{e9}on" })))
        );
        assert_eq!(
            input("{'big': 18446744073709551615, 'whole': 4611686018427387904.0}"),
            Ok(Some(json!({ "big": u64::MAX, "whole": 4611686018427387904i64 })))
        );
        assert!(input("{'x': 1e300}").is_err_and(|message| message.contains("too large")));
        assert!(input("{1: 'a', '1': 'b'}").is_err_and(|message| message.contains("the key `1` twice")));
        assert_eq!(input("{'b': 1, 'a': 2}").unwrap().unwrap().to_string(), r#"{"a":2,"b":1}"#);
        assert_eq!(input("{'gone': entry.missing}"), Ok(None));
        assert_eq!(input("{}"), Ok(None));
        assert_eq!(input("entry.name"), Ok(Some(json!("N\u{e9}on"))));
        assert_eq!(input("'  '"), Ok(None));
        assert_eq!(input("entry.missing"), Ok(None));
        assert_eq!(input("12"), Ok(Some(json!("12"))));
        assert!(
            input("{'x': range(3)}").is_err_and(|message| message.contains("can hold only")),
            "{:?}",
            input("{'x': range(3)}")
        );
        let long = format!("{{'x': '{}'}}", "a".repeat(INPUT_LIMIT));
        assert!(input(&long).is_err_and(|message| message.contains("longer than")));
    }

    #[test]
    fn text_media_jobs_keep_their_hash_and_kinds_differ() {
        let key = json!({ "voice": "a" });
        assert_eq!(job_hash("voice", Kind::Media, &key, &json!("⚛️")), media_hash("voice", &key, "⚛️"));
        assert_ne!(
            job_hash("voice", Kind::Data, &key, &json!("⚛️")),
            job_hash("voice", Kind::Media, &key, &json!("⚛️"))
        );
        assert_ne!(
            job_hash("voice", Kind::Media, &key, &json!({ "a": 1 })),
            job_hash("voice", Kind::Media, &key, &json!(r#"{"a":1}"#))
        );
        assert_eq!(
            job_hash("voice", Kind::Data, &key, &json!({ "a": 1, "b": 2 })),
            job_hash("voice", Kind::Data, &key, &json!({ "b": 2, "a": 1 }))
        );
    }

    #[test]
    fn plan_results_are_checked() {
        let planned = parse_plan(json!({ "ext": "ogg", "key": [1, "a"], "units": 3, "unit": "characters" })).unwrap();
        assert_eq!(planned.ext.as_deref(), Some("ogg"));
        assert_eq!(planned.key, Some(json!([1, "a"])));
        assert_eq!(parse_plan(json!([])).unwrap(), Planned::default());
        assert_eq!(parse_plan(json!({ "units": 3.0 })).unwrap().units, Some(3));
        for (bad, message) in [
            (json!("x"), "must return a table"),
            (json!({ "ext": "OGG" }), "`ext` must be"),
            (json!({ "ext": "../x" }), "`ext` must be"),
            (json!({ "ext": "json" }), "not `json`"),
            (json!({ "units": -1 }), "`units` must be"),
            (json!({ "units": 1.5 }), "`units` must be"),
            (json!({ "unit": "" }), "`unit` must be"),
            (json!({ "unit": "\u{1b}[2J" }), "`unit` must be"),
            (json!({ "units": 1e20 }), "`units` must be"),
            (json!({ "key": "x".repeat(70000) }), "`key` is longer than"),
            (json!({ "key": "a\u{fffd}" }), "valid UTF-8"),
            (json!({ "colour": 1 }), "unknown key `colour`"),
        ] {
            assert!(parse_plan(bad).unwrap_err().contains(message), "{message}");
        }
    }

    #[test]
    fn the_hash_ignores_key_order_but_not_the_input() {
        let one = media_hash("voice", &json!({ "a": 1, "b": { "c": 2, "d": 3 } }), "⚛️");
        let two = media_hash("voice", &json!({ "b": { "d": 3, "c": 2 }, "a": 1 }), "⚛️");
        assert_eq!(one, two);
        assert_eq!(one.len(), 32);
        assert_ne!(one, media_hash("voice", &json!({ "a": 1, "b": { "c": 2, "d": 3 } }), "🧪"));
        assert_ne!(media_hash("ab", &json!("c"), ""), media_hash("a", &json!("bc"), ""));
    }
}
