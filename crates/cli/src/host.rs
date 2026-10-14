//! The `janky` table that every plugin sees, and the conversion of values between Lua, JSON and
//! minijinja.
//!
//! [`install`] adds the host functions that do no I/O: `janky.html`, `janky.escape`,
//! `janky.log`, `janky.json.encode` and `janky.json.decode`. The functions that do I/O
//! (`janky.http.request`, `janky.http.download`, `janky.secret`, `janky.wait`, `janky.cache`
//! and `janky.fs`) come from [`crate::runner`], and exist only while generators run.
//! [`crate::plugin`] sets `janky.variables` before each call.
//!
//! A plain string from a plugin is text, and janky escapes it. Only an [`Html`] value, which
//! `janky.html` makes, goes into a template as trusted HTML. `..` on an [`Html`] value is an
//! error, so a plugin must escape the text parts and mark the complete result once.
//!
//! All conversions follow the same rules. A table with the keys `1..n` and no others is a list.
//! An empty table is a list. Other tables are maps whose keys are strings or integers, and an
//! integer key becomes text. Conversions read tables with raw access, so they run no plugin
//! metamethods. One conversion may hold at most [`VALUE_LIMIT`] values, [`TEXT_LIMIT`] bytes of
//! text and [`DEPTH_LIMIT`] levels of tables. These limits keep the memory outside the Lua limit
//! small.

use std::ffi::c_void;

use mlua::{Lua, MetaMethod, Table, UserData, UserDataMethods, Value as LuaValue};
use serde::de::{DeserializeSeed, Deserializer, Error as _, MapAccess, SeqAccess, Visitor};
use serde_json::Value as Json;

use crate::render::escape_html;

/// The most values (table keys included) in one conversion.
pub const VALUE_LIMIT: usize = 1_000_000;
/// The most bytes of text in one conversion. It is also the longest output of `janky.escape`
/// and `janky.json.encode`.
pub const TEXT_LIMIT: usize = 16 * 1024 * 1024;
/// The deepest nesting of tables in one conversion.
pub const DEPTH_LIMIT: usize = 128;

const CONCAT: &str = "marked HTML cannot be joined with `..`: escape the text parts with janky.escape, join \
                      them, and mark the whole result once with janky.html";

/// A Lua string that `janky.html` marked as trusted HTML.
///
/// [`from_lua`] gives it to minijinja as a safe string, which the template does not escape.
/// `tostring` gives the string back. `..` with any value is an error.
#[derive(Debug, Clone)]
pub struct Html(pub mlua::LuaString);

impl UserData for Html {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_function(MetaMethod::Concat, |_, _: (LuaValue, LuaValue)| {
            Err::<(), _>(mlua::Error::runtime(CONCAT))
        });
        methods.add_meta_method(MetaMethod::ToString, |_, html, ()| Ok(html.0.clone()));
    }
}

/// Sets the global `janky` table, with `log` as `janky.log`.
///
/// `janky.escape` escapes `& < > " '`, and replaces bytes that are not UTF-8 with U+FFFD.
/// `janky.json.encode` follows the rules of [`lua_to_json`]. `janky.json.decode` builds the Lua
/// values directly, so they count against the Lua memory limit. JSON `null` becomes `nil`.
pub fn install(lua: &Lua, log: mlua::Function) -> mlua::Result<()> {
    let janky = lua.create_table()?;
    janky.set("html", lua.create_function(|_, text: mlua::LuaString| Ok(Html(text)))?)?;
    janky.set(
        "escape",
        lua.create_function(|lua, text: mlua::LuaString| {
            let size: usize = text
                .as_bytes()
                .utf8_chunks()
                .map(|chunk| {
                    let valid: usize = chunk
                        .valid()
                        .bytes()
                        .map(|byte| match byte {
                            b'&' => 5,
                            b'<' | b'>' => 4,
                            b'"' => 6,
                            b'\'' => 5,
                            _ => 1,
                        })
                        .sum();
                    valid + if chunk.invalid().is_empty() { 0 } else { 3 }
                })
                .sum();
            if size > TEXT_LIMIT {
                return Err(mlua::Error::runtime(format!("the escaped text is longer than {TEXT_LIMIT} bytes")));
            }
            lua.create_string(escape_html(&text.to_string_lossy()))
        })?,
    )?;
    janky.set("log", log)?;
    let json = lua.create_table()?;
    json.set(
        "encode",
        lua.create_function(|lua, value: LuaValue| {
            let json = lua_to_json(&value).map_err(mlua::Error::runtime)?;
            let mut out = Limited(Vec::new());
            serde_json::to_writer(&mut out, &json)
                .map_err(|_| mlua::Error::runtime(format!("the encoded JSON is longer than {TEXT_LIMIT} bytes")))?;
            lua.create_string(out.0)
        })?,
    )?;
    json.set(
        "decode",
        lua.create_function(|lua, text: mlua::LuaString| {
            let bytes = text.as_bytes();
            let mut deserializer = serde_json::Deserializer::from_slice(&bytes);
            let value = LuaSeed(lua)
                .deserialize(&mut deserializer)
                .and_then(|value| deserializer.end().map(|()| value))
                .map_err(|error| mlua::Error::runtime(format!("bad JSON: {error}")))?;
            Ok(value)
        })?,
    )?;
    janky.set("json", json)?;
    lua.globals().set("janky", janky)
}

struct Limited(Vec<u8>);

impl std::io::Write for Limited {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.0.len() + bytes.len() > TEXT_LIMIT {
            return Err(std::io::Error::other("too long"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn lossy_len(bytes: &[u8]) -> usize {
    bytes.utf8_chunks().map(|chunk| chunk.valid().len() + if chunk.invalid().is_empty() { 0 } else { 3 }).sum()
}

struct LuaSeed<'a>(&'a Lua);

impl<'de> DeserializeSeed<'de> for LuaSeed<'_> {
    type Value = LuaValue;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<LuaValue, D::Error> {
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for LuaSeed<'_> {
    type Value = LuaValue;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("a JSON value")
    }

    fn visit_unit<E: serde::de::Error>(self) -> Result<LuaValue, E> {
        Ok(LuaValue::Nil)
    }

    fn visit_bool<E: serde::de::Error>(self, value: bool) -> Result<LuaValue, E> {
        Ok(LuaValue::Boolean(value))
    }

    fn visit_i64<E: serde::de::Error>(self, value: i64) -> Result<LuaValue, E> {
        Ok(LuaValue::Integer(value))
    }

    fn visit_u64<E: serde::de::Error>(self, value: u64) -> Result<LuaValue, E> {
        Ok(i64::try_from(value).map_or(LuaValue::Number(value as f64), LuaValue::Integer))
    }

    fn visit_f64<E: serde::de::Error>(self, value: f64) -> Result<LuaValue, E> {
        Ok(LuaValue::Number(value))
    }

    fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<LuaValue, E> {
        self.0.create_string(value).map(LuaValue::String).map_err(E::custom)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<LuaValue, A::Error> {
        let table = self.0.create_table().map_err(A::Error::custom)?;
        let mut index = 1;
        while let Some(item) = seq.next_element_seed(LuaSeed(self.0))? {
            table.raw_set(index, item).map_err(A::Error::custom)?;
            index += 1;
        }
        Ok(LuaValue::Table(table))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<LuaValue, A::Error> {
        let table = self.0.create_table().map_err(A::Error::custom)?;
        while let Some(key) = map.next_key::<String>()? {
            let item = map.next_value_seed(LuaSeed(self.0))?;
            table.raw_set(key, item).map_err(A::Error::custom)?;
        }
        Ok(LuaValue::Table(table))
    }
}

struct Budget {
    values: usize,
    text: usize,
    path: Vec<*const c_void>,
}

impl Budget {
    fn new() -> Self {
        Self { values: 0, text: 0, path: Vec::new() }
    }

    fn spend(&mut self, text: usize) -> Result<(), String> {
        self.values += 1;
        self.text += text;
        if self.values > VALUE_LIMIT {
            return Err(format!("the value has more than {VALUE_LIMIT} parts"));
        }
        if self.text > TEXT_LIMIT {
            return Err(format!("the value has more than {TEXT_LIMIT} bytes of text"));
        }
        Ok(())
    }

    fn enter(&mut self, table: &Table) -> Result<(), String> {
        if self.path.len() >= DEPTH_LIMIT {
            return Err(format!("the value is nested more than {DEPTH_LIMIT} levels deep"));
        }
        let pointer = table.to_pointer();
        if self.path.contains(&pointer) {
            return Err("the table contains itself".into());
        }
        self.path.push(pointer);
        Ok(())
    }
}

fn sequence_length(table: &Table) -> Option<usize> {
    let mut count = 0;
    for pair in table.pairs::<LuaValue, LuaValue>() {
        let (key, _) = pair.ok()?;
        match key {
            LuaValue::Integer(index) if index >= 1 => count += 1,
            _ => return None,
        }
    }
    (count == table.raw_len()).then_some(count)
}

fn table_key(key: &LuaValue, budget: &mut Budget) -> Result<String, String> {
    let key = match key {
        LuaValue::String(text) => text.to_string_lossy(),
        LuaValue::Integer(number) => number.to_string(),
        other => return Err(format!("a table key must be a string or an integer, not {}", other.type_name())),
    };
    budget.spend(key.len())?;
    Ok(key)
}

/// Converts a Lua value to JSON, for `janky.json.encode`, `janky.cache.put`, the result of
/// `plan` and the `value` of a data job.
///
/// Map keys are sorted. Lua seeds its string hashes for each state, so the order of `pairs`
/// changes between runs, and unsorted keys would change hashes and fields from one run to the
/// next.
///
/// An [`Html`] value becomes its string, and invalid UTF-8 becomes U+FFFD. The error message
/// says what is wrong: NaN or infinity, a table that contains itself, a limit, two keys that
/// become the same text, a key that is not a string or an integer, or a value of another type
/// (for example a function or a secret handle).
pub fn lua_to_json(value: &LuaValue) -> Result<Json, String> {
    lua_to_json_in(value, &mut Budget::new())
}

fn lua_to_json_in(value: &LuaValue, budget: &mut Budget) -> Result<Json, String> {
    budget.spend(0)?;
    Ok(match value {
        LuaValue::Nil => Json::Null,
        LuaValue::Boolean(flag) => Json::Bool(*flag),
        LuaValue::Integer(number) => Json::from(*number),
        LuaValue::Number(number) => serde_json::Number::from_f64(*number)
            .map(Json::Number)
            .ok_or_else(|| format!("{number} is not a finite number"))?,
        LuaValue::String(text) => {
            budget.spend(lossy_len(&text.as_bytes()))?;
            Json::String(text.to_string_lossy())
        },
        LuaValue::UserData(data) => match data.borrow::<Html>() {
            Ok(html) => {
                budget.spend(lossy_len(&html.0.as_bytes()))?;
                Json::String(html.0.to_string_lossy())
            },
            Err(_) => return Err("this value cannot be converted".into()),
        },
        LuaValue::Table(table) => {
            budget.enter(table)?;
            let json = match sequence_length(table) {
                Some(length) => {
                    let mut items = Vec::new();
                    for index in 1..=length {
                        let item: LuaValue = table.raw_get(index).map_err(|error| error.to_string())?;
                        items.push(lua_to_json_in(&item, budget)?);
                    }
                    Json::Array(items)
                },
                None => {
                    let mut object = serde_json::Map::new();
                    for pair in table.pairs::<LuaValue, LuaValue>() {
                        let (key, item) = pair.map_err(|error| error.to_string())?;
                        let key = table_key(&key, budget)?;
                        if object.insert(key.clone(), lua_to_json_in(&item, budget)?).is_some() {
                            return Err(format!("two table keys become the key `{key}`"));
                        }
                    }
                    object.sort_keys();
                    Json::Object(object)
                },
            };
            budget.path.pop();
            json
        },
        other => return Err(format!("a {} cannot be converted", other.type_name())),
    })
}

/// Converts a template value to Lua, for the arguments of plugin calls and for `janky.variables`.
/// The runner also uses it for the values of `janky.cache.get` and `janky.fs.json_items`.
///
/// Undefined and none become `nil`. A safe string becomes a plain string, so a plugin cannot
/// tell text from HTML. Map keys become text. A value of another kind becomes its text. Values
/// nested deeper than [`DEPTH_LIMIT`] are an error.
pub fn to_lua(lua: &Lua, value: &minijinja::Value) -> mlua::Result<LuaValue> {
    to_lua_in(lua, value, 0)
}

fn to_lua_in(lua: &Lua, value: &minijinja::Value, depth: usize) -> mlua::Result<LuaValue> {
    use minijinja::value::ValueKind;
    if depth > DEPTH_LIMIT {
        return Err(mlua::Error::runtime(format!("the value is nested more than {DEPTH_LIMIT} levels deep")));
    }
    Ok(match value.kind() {
        ValueKind::Undefined | ValueKind::None => LuaValue::Nil,
        ValueKind::Bool => LuaValue::Boolean(value.is_true()),
        ValueKind::Number => match i64::try_from(value.clone()) {
            Ok(integer) => LuaValue::Integer(integer),
            Err(_) => LuaValue::Number(f64::try_from(value.clone()).unwrap_or(f64::NAN)),
        },
        ValueKind::Seq | ValueKind::Iterable => {
            let table = lua.create_table()?;
            for (index, item) in value.try_iter().map_err(mlua::Error::external)?.enumerate() {
                table.raw_set(index + 1, to_lua_in(lua, &item, depth + 1)?)?;
            }
            LuaValue::Table(table)
        },
        ValueKind::Map => {
            let table = lua.create_table()?;
            for key in value.try_iter().map_err(mlua::Error::external)? {
                let item = value.get_item(&key).map_err(mlua::Error::external)?;
                table.raw_set(key.to_string(), to_lua_in(lua, &item, depth + 1)?)?;
            }
            LuaValue::Table(table)
        },
        _ => LuaValue::String(lua.create_string(value.to_string())?),
    })
}

/// Converts the result of a plugin filter or function to a template value.
///
/// An [`Html`] value becomes a safe string. A plain string stays text, which the template
/// escapes. Maps are sorted by key. The errors are those of [`lua_to_json`], except that NaN
/// and infinity are allowed.
pub fn from_lua(value: &LuaValue) -> Result<minijinja::Value, String> {
    from_lua_in(value, &mut Budget::new())
}

fn from_lua_in(value: &LuaValue, budget: &mut Budget) -> Result<minijinja::Value, String> {
    budget.spend(0)?;
    Ok(match value {
        LuaValue::Nil => minijinja::Value::from(()),
        LuaValue::Boolean(flag) => minijinja::Value::from(*flag),
        LuaValue::Integer(number) => minijinja::Value::from(*number),
        LuaValue::Number(number) => minijinja::Value::from(*number),
        LuaValue::String(text) => {
            budget.spend(lossy_len(&text.as_bytes()))?;
            minijinja::Value::from(text.to_string_lossy())
        },
        LuaValue::UserData(data) => match data.borrow::<Html>() {
            Ok(html) => {
                budget.spend(lossy_len(&html.0.as_bytes()))?;
                minijinja::Value::from_safe_string(html.0.to_string_lossy())
            },
            Err(_) => return Err("this value cannot be returned to a template".into()),
        },
        LuaValue::Table(table) => {
            budget.enter(table)?;
            let value = match sequence_length(table) {
                Some(length) => {
                    let mut items = Vec::new();
                    for index in 1..=length {
                        let item: LuaValue = table.raw_get(index).map_err(|error| error.to_string())?;
                        items.push(from_lua_in(&item, budget)?);
                    }
                    minijinja::Value::from(items)
                },
                None => {
                    let mut entries = std::collections::BTreeMap::new();
                    for pair in table.pairs::<LuaValue, LuaValue>() {
                        let (key, item) = pair.map_err(|error| error.to_string())?;
                        let key = table_key(&key, budget)?;
                        if entries.insert(key.clone(), from_lua_in(&item, budget)?).is_some() {
                            return Err(format!("two table keys become the key `{key}`"));
                        }
                    }
                    minijinja::Value::from(entries)
                },
            };
            budget.path.pop();
            value
        },
        other => return Err(format!("a {} cannot be returned to a template", other.type_name())),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lua::Sandbox;

    fn run(sandbox: &Sandbox, source: &str) -> mlua::Result<LuaValue> {
        sandbox.exec(source, "host.lua")
    }

    #[test]
    fn marked_html_cannot_be_concatenated() {
        let sandbox = Sandbox::new().unwrap();
        for source in [
            "return janky.html('<b>') .. 'x'",
            "return 'x' .. janky.html('<b>')",
            "return janky.html('<b>') .. janky.html('</b>')",
        ] {
            let message = run(&sandbox, source).unwrap_err().to_string();
            assert!(message.contains("marked HTML cannot be joined"), "{message}");
        }
        let value = run(&sandbox, "return tostring(janky.html('<b>👩‍🔬</b>'))").unwrap();
        assert_eq!(value.to_string().unwrap(), "<b>👩‍🔬</b>");
    }

    #[test]
    fn escape_builds_html_around_text() {
        let sandbox = Sandbox::new().unwrap();
        let value =
            run(&sandbox, "return janky.html(janky.escape('a<b') .. '<b>' .. janky.escape('\"&') .. '</b>')").unwrap();
        assert_eq!(from_lua(&value).unwrap().to_string(), "a&lt;b<b>&quot;&amp;</b>");
        assert!(from_lua(&value).unwrap().is_safe());
    }

    #[test]
    fn json_round_trips() {
        let sandbox = Sandbox::new().unwrap();
        let value = run(
            &sandbox,
            "local t = janky.json.decode('{\"a\":[1,2.5,\"🧪\"],\"b\":{\"c\":true},\"d\":null}') \
             return janky.json.encode({ list = t.a, nested = t.b.c, empty = {} })",
        )
        .unwrap();
        let json: Json = serde_json::from_str(&value.to_string().unwrap()).unwrap();
        assert_eq!(json, serde_json::json!({ "list": [1, 2.5, "🧪"], "nested": true, "empty": [] }));
        let message = run(&sandbox, "return janky.json.encode({ x = 1/0 })").unwrap_err().to_string();
        assert!(message.contains("not a finite number"), "{message}");
        let message = run(&sandbox, "return janky.json.decode('{')").unwrap_err().to_string();
        assert!(message.contains("bad JSON"), "{message}");
    }

    #[test]
    fn encoded_keys_are_sorted() {
        let sandbox = Sandbox::new().unwrap();
        let value = run(
            &sandbox,
            "return janky.json.encode({ voice = 1, model = 2, speed = 3, b = { z = 1, a = 2, m = { y = 1, x = 2 } } })",
        )
        .unwrap();
        assert_eq!(
            value.to_string().unwrap(),
            r#"{"b":{"a":2,"m":{"x":2,"y":1},"z":1},"model":2,"speed":3,"voice":1}"#
        );
    }

    #[test]
    fn log_shares_the_print_log() {
        let sandbox = Sandbox::new().unwrap();
        run(&sandbox, "janky.log('a', 1) print('b')").unwrap();
        assert_eq!(sandbox.take_log(), ["a\t1", "b"]);
    }

    #[test]
    fn values_convert_both_ways() {
        let sandbox = Sandbox::new().unwrap();
        let input = minijinja::Value::from(minijinja::value::Serde(
            serde_json::json!({ "n": 2, "list": ["x", 1.5], "flag": true }),
        ));
        let lua = to_lua(&sandbox.lua, &input).unwrap();
        sandbox.lua.globals().set("input", lua).unwrap();
        let safe = to_lua(&sandbox.lua, &minijinja::Value::from_safe_string("<i>x</i>".into())).unwrap();
        assert!(matches!(safe, LuaValue::String(_)));
        let list = from_lua(&run(&sandbox, "return { input.n + 1, input.list[1], input.flag }").unwrap()).unwrap();
        assert_eq!(list.to_string(), "[3, 'x', True]");
        let map = from_lua(&run(&sandbox, "return { html = janky.html('<b>'), nested = { 'y' } }").unwrap()).unwrap();
        assert!(map.get_attr("html").unwrap().is_safe());
        assert_eq!(map.get_attr("nested").unwrap().to_string(), "['y']");
        let message = from_lua(&run(&sandbox, "return { f = print }").unwrap()).unwrap_err();
        assert!(message.contains("function cannot be returned"), "{message}");
    }

    #[test]
    fn tables_that_contain_themselves_or_grow_too_large_are_errors() {
        let sandbox = Sandbox::new().unwrap();
        let message = run(&sandbox, "local t = {} t[1] = t return janky.json.encode(t)").unwrap_err().to_string();
        assert!(message.contains("the table contains itself"), "{message}");
        let shared = "local t = {} for i = 1, 30 do t = { t, t } end return t";
        let message = run(&sandbox, &format!("{} ", shared.replace("return t", "return janky.json.encode(t)")))
            .unwrap_err()
            .to_string();
        assert!(message.contains("more than 1000000 parts"), "{message}");
        let message = from_lua(&run(&sandbox, shared).unwrap()).unwrap_err();
        assert!(message.contains("more than 1000000 parts"), "{message}");
        let message = run(&sandbox, "return janky.escape(string.rep('<', 5 * 1024 * 1024))").unwrap_err().to_string();
        assert!(message.contains("longer than 16777216 bytes"), "{message}");
        for source in [
            "return janky.escape(string.rep('\\255', 6 * 1024 * 1024))",
            "return janky.json.encode(string.rep('\\1', 3 * 1024 * 1024))",
        ] {
            let message = run(&sandbox, source).unwrap_err().to_string();
            assert!(message.contains("longer than 16777216 bytes"), "{source}: {message}");
        }
        let message = run(&sandbox, "return janky.json.encode({ string.rep('\\255', 6 * 1024 * 1024) })")
            .unwrap_err()
            .to_string();
        assert!(message.contains("16777216"), "{message}");
        let value = run(&sandbox, "local x = {1} return janky.json.encode({ x, x })").unwrap();
        assert_eq!(value.to_string().unwrap(), "[[1],[1]]");
    }

    #[test]
    fn deep_template_values_are_errors() {
        let sandbox = Sandbox::new().unwrap();
        let mut value = minijinja::Value::from(1);
        for _ in 0..200 {
            value = minijinja::Value::from(vec![value]);
        }
        let message = to_lua(&sandbox.lua, &value).unwrap_err().to_string();
        assert!(message.contains("nested more than 128 levels"), "{message}");
    }

    #[test]
    fn keys_that_become_the_same_text_are_errors() {
        let sandbox = Sandbox::new().unwrap();
        let value = run(&sandbox, "return { [1] = 'a', ['1'] = 'b', x = 1 }").unwrap();
        assert_eq!(from_lua(&value).unwrap_err(), "two table keys become the key `1`");
        let message =
            run(&sandbox, "return janky.json.encode({ ['\\255'] = 1, ['\\254'] = 2 })").unwrap_err().to_string();
        assert!(message.contains("two table keys become the key `\u{fffd}`"), "{message}");
    }

    #[test]
    fn deeply_nested_tables_are_errors() {
        let sandbox = Sandbox::new().unwrap();
        let deep = "local t = {} for i = 1, 100000 do t = { t } end return t";
        let message = from_lua(&run(&sandbox, deep).unwrap()).unwrap_err();
        assert!(message.contains("nested more than 128 levels"), "{message}");
        let source = deep.replace("return t", "return janky.json.encode(t)");
        let message = run(&sandbox, &source).unwrap_err().to_string();
        assert!(message.contains("nested more than 128 levels"), "{message}");
        let message =
            run(&sandbox, &format!("return janky.json.decode('{}')", "[".repeat(200))).unwrap_err().to_string();
        assert!(message.contains("bad JSON"), "{message}");
    }

    #[test]
    fn decoded_json_counts_against_the_lua_memory_limit() {
        let sandbox = Sandbox::new().unwrap();
        let message = run(&sandbox, "local s = '[' .. string.rep('[0],', 3000000) .. '0]' return janky.json.decode(s)")
            .unwrap_err()
            .to_string();
        assert!(message.contains("memory"), "{message}");
    }
}
