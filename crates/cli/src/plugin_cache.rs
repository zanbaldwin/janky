//! The plugin cache behind `janky.cache.get` and `janky.cache.put`.
//!
//! Each plugin instance has its own SQLite file, `.janky/cache/plugins/<instance>/cache.sqlite`,
//! with one row for each key. A value is stored as JSON text, so text, numbers, `true` and
//! `false`, lists and maps come back as they were stored. The instance name comes from
//! `deck.toml`, so a plugin cannot open another instance's cache.
//!
//! The file uses WAL mode and a busy timeout of 1 second, so a second process waits a little
//! instead of failing, without reaching the time limit of a plugin call. Each `put` is its own
//! transaction, unless [`PluginCache::begin`] opened one. [`crate::runner`] does this for
//! `prepare`, so all its writes are kept or lost together. A committed `put` survives a crash
//! of janky, but not always a power cut.
//!
//! A changed file must not change what janky does: janky opens only a regular file and does not
//! follow a link, uses SQLite's defensive mode, `trusted_schema = OFF` and `cell_size_check`,
//! and refuses a file whose schema is not exactly the `entries` table.

use std::path::Path;
use std::time::Duration;

use rusqlite::config::DbConfig;
use rusqlite::{Connection, OpenFlags, OptionalExtension};

const SCHEMA: &str =
    "CREATE TABLE IF NOT EXISTS entries (key TEXT PRIMARY KEY NOT NULL, value TEXT NOT NULL) WITHOUT ROWID";

/// The longest key, in bytes.
pub const KEY_LIMIT: usize = 1024;
/// The longest value, in bytes of JSON.
pub const VALUE_LIMIT: usize = 1024 * 1024;
/// The most bytes of keys and values that one instance can store.
pub const NAMESPACE_LIMIT: u64 = 256 * 1024 * 1024;

/// The open cache of one plugin instance.
pub struct PluginCache {
    connection: Connection,
    bytes: u64,
    limit: u64,
    transaction: bool,
}

impl PluginCache {
    /// Opens the cache of `instance` in the project at `root`, and makes it when it is missing.
    ///
    /// It fails when the directory is not safe to use (see [`crate::cache::plugin_cache_file`])
    /// or when SQLite cannot open the file. The error is a message for the author.
    pub fn open(root: &Path, instance: &str) -> Result<Self, String> {
        let path = crate::cache::plugin_cache_file(root, instance)?;
        Self::open_at(&path, NAMESPACE_LIMIT)
    }

    /// Opens the cache at `path` with a namespace limit of `limit` bytes. Tests use it.
    pub fn open_at(path: &Path, limit: u64) -> Result<Self, String> {
        let failed = |error: rusqlite::Error| format!("cannot open the plugin cache `{}`: {error}", path.display());
        if std::fs::symlink_metadata(path).is_ok_and(|metadata| !metadata.is_file()) {
            return Err(format!("the plugin cache `{}` must be a regular file", path.display()));
        }
        let connection = Connection::open_with_flags(path, OpenFlags::default() | OpenFlags::SQLITE_OPEN_NOFOLLOW)
            .map_err(failed)?;
        connection.set_db_config(DbConfig::SQLITE_DBCONFIG_DEFENSIVE, true).map_err(failed)?;
        connection.busy_timeout(Duration::from_secs(1)).map_err(failed)?;
        connection
            .execute_batch(&format!(
                "PRAGMA trusted_schema = OFF;
                 PRAGMA cell_size_check = ON;
                 PRAGMA journal_mode = WAL;
                 PRAGMA synchronous = NORMAL;
                 {SCHEMA};"
            ))
            .map_err(failed)?;
        let schema: Vec<(String, String, Option<String>)> = connection
            .prepare("SELECT type, name, sql FROM sqlite_schema")
            .and_then(|mut statement| {
                statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?.collect()
            })
            .map_err(failed)?;
        let expected = SCHEMA.replace("IF NOT EXISTS ", "");
        if schema != [("table".to_string(), "entries".to_string(), Some(expected))] {
            return Err(format!("the plugin cache `{}` has an unknown schema: delete it", path.display()));
        }
        let bytes: i64 = connection
            .query_row(
                "SELECT coalesce(sum(length(CAST(key AS BLOB)) + length(CAST(value AS BLOB))), 0) FROM entries",
                [],
                |row| row.get(0),
            )
            .map_err(failed)?;
        Ok(Self {
            connection,
            bytes: bytes.max(0) as u64,
            limit,
            transaction: false,
        })
    }

    /// Starts a transaction, unless one is open. Until [`finish`](Self::finish), other
    /// processes do not see the writes, and a crash keeps none of them.
    pub fn begin(&mut self) -> Result<(), String> {
        if !self.transaction {
            self.connection
                .execute_batch("BEGIN IMMEDIATE")
                .map_err(|error| format!("cannot start a plugin cache transaction: {error}"))?;
            self.transaction = true;
        }
        Ok(())
    }

    /// Ends the open transaction: keeps its writes when `commit` is true, else removes them. It
    /// does nothing when no transaction is open.
    pub fn finish(&mut self, commit: bool) -> Result<(), String> {
        if !self.transaction {
            return Ok(());
        }
        self.transaction = false;
        let statement = if commit { "COMMIT" } else { "ROLLBACK" };
        let ended = self.connection.execute_batch(statement);
        if ended.is_err() {
            let _ = self.connection.execute_batch("ROLLBACK");
        }
        if !commit || ended.is_err() {
            let bytes: i64 = self
                .connection
                .query_row(
                    "SELECT coalesce(sum(length(CAST(key AS BLOB)) + length(CAST(value AS BLOB))), 0) FROM entries",
                    [],
                    |row| row.get(0),
                )
                .map_err(|error| format!("cannot read the plugin cache: {error}"))?;
            self.bytes = bytes.max(0) as u64;
        }
        ended.map_err(|error| format!("cannot end the plugin cache transaction: {error}"))
    }

    /// Returns the stored value of `key` as JSON, or `None` when there is none.
    pub fn get(&self, key: &str) -> Result<Option<serde_json::Value>, String> {
        check_key(key)?;
        let text: Option<String> = self
            .connection
            .query_row("SELECT value FROM entries WHERE key = ?1", [key], |row| row.get(0))
            .optional()
            .map_err(|error| format!("cannot read the plugin cache: {error}"))?;
        text.map(|text| {
            serde_json::from_str(&text).map_err(|error| format!("the plugin cache holds a broken value: {error}"))
        })
        .transpose()
    }

    /// Stores `value` under `key`, or removes the key when `value` is `None`.
    ///
    /// It fails when the key is empty or longer than [`KEY_LIMIT`], when the value is longer
    /// than [`VALUE_LIMIT`] as JSON, when the value is nested too deeply for serde_json to read
    /// back, or when the instance would store more than its namespace limit.
    pub fn put(&mut self, key: &str, value: Option<&serde_json::Value>) -> Result<(), String> {
        check_key(key)?;
        let failed = |error: rusqlite::Error| format!("cannot write the plugin cache: {error}");
        let old: Option<i64> = self
            .connection
            .query_row(
                "SELECT length(CAST(key AS BLOB)) + length(CAST(value AS BLOB)) FROM entries WHERE key = ?1",
                [key],
                |row| row.get(0),
            )
            .optional()
            .map_err(failed)?;
        let old = old.unwrap_or(0).max(0) as u64;
        let Some(value) = value else {
            self.connection.execute("DELETE FROM entries WHERE key = ?1", [key]).map_err(failed)?;
            self.bytes = self.bytes.saturating_sub(old);
            return Ok(());
        };
        let text = value.to_string();
        if text.len() > VALUE_LIMIT {
            return Err(format!("a plugin cache value has at most {VALUE_LIMIT} bytes as JSON"));
        }
        if serde_json::from_str::<serde_json::Value>(&text).is_err() {
            return Err("a plugin cache value is nested too deeply to read back".into());
        }
        let bytes = self.bytes.saturating_sub(old) + (key.len() + text.len()) as u64;
        if bytes > self.limit {
            return Err(format!("the plugin cache of this instance would hold more than {} bytes", self.limit));
        }
        self.connection
            .execute("INSERT OR REPLACE INTO entries (key, value) VALUES (?1, ?2)", (key, &text))
            .map_err(failed)?;
        self.bytes = bytes;
        Ok(())
    }
}

fn check_key(key: &str) -> Result<(), String> {
    if key.is_empty() || key.len() > KEY_LIMIT {
        return Err(format!("a plugin cache key has 1 to {KEY_LIMIT} bytes"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_changed_or_foreign_file_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cache.sqlite");
        let other = Connection::open(&path).unwrap();
        other
            .execute_batch(&format!(
                "{SCHEMA}; CREATE TRIGGER t AFTER INSERT ON entries BEGIN DELETE FROM entries; END;"
            ))
            .unwrap();
        drop(other);
        assert!(PluginCache::open_at(&path, NAMESPACE_LIMIT).err().unwrap().contains("unknown schema"));
        let link = dir.path().join("link.sqlite");
        std::os::unix::fs::symlink(&path, &link).unwrap();
        assert!(PluginCache::open_at(&link, NAMESPACE_LIMIT).is_err());
        let text = dir.path().join("text.sqlite");
        std::fs::write(&text, "not a database").unwrap();
        assert!(PluginCache::open_at(&text, NAMESPACE_LIMIT).is_err());
        assert!(PluginCache::open_at(dir.path(), NAMESPACE_LIMIT).err().unwrap().contains("regular file"));
    }

    #[test]
    fn values_keep_their_type_and_nil_removes_a_key() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cache.sqlite");
        let mut cache = PluginCache::open_at(&path, NAMESPACE_LIMIT).unwrap();
        for value in [
            json!("⚛️"),
            json!({ "pos": "noun", "n": [1, 2.5] }),
            json!(3),
            json!(true),
        ] {
            cache.put("k", Some(&value)).unwrap();
            assert_eq!(cache.get("k").unwrap(), Some(value));
        }
        cache.put("k", None).unwrap();
        assert_eq!(cache.get("k").unwrap(), None);
        cache.put("kept", Some(&json!("x"))).unwrap();
        drop(cache);
        assert_eq!(PluginCache::open_at(&path, NAMESPACE_LIMIT).unwrap().get("kept").unwrap(), Some(json!("x")));
    }

    #[test]
    fn a_rolled_back_transaction_keeps_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cache.sqlite");
        let mut cache = PluginCache::open_at(&path, 1000).unwrap();
        cache.put("kept", Some(&json!(1))).unwrap();
        cache.begin().unwrap();
        cache.put("gone", Some(&json!("x".repeat(500)))).unwrap();
        cache.finish(false).unwrap();
        assert_eq!(cache.get("gone").unwrap(), None);
        cache.put("fits", Some(&json!("y".repeat(900)))).unwrap();
        cache.begin().unwrap();
        cache.put("committed", Some(&json!(2))).unwrap();
        cache.finish(true).unwrap();
        drop(cache);
        let cache = PluginCache::open_at(&path, 1000).unwrap();
        assert_eq!(cache.get("kept").unwrap(), Some(json!(1)));
        assert_eq!(cache.get("committed").unwrap(), Some(json!(2)));
    }

    #[test]
    fn keys_values_and_the_namespace_are_limited() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cache.sqlite");
        let mut cache = PluginCache::open_at(&path, 100).unwrap();
        assert!(cache.put("", Some(&json!(1))).is_err());
        assert!(cache.get(&"k".repeat(KEY_LIMIT + 1)).is_err());
        assert!(cache.put("k", Some(&json!("a".repeat(VALUE_LIMIT)))).is_err());
        let mut deep = json!(1);
        for _ in 0..130 {
            deep = json!([deep]);
        }
        assert!(cache.put("k", Some(&deep)).unwrap_err().contains("nested too deeply"));
        cache.put("a", Some(&json!("x".repeat(60)))).unwrap();
        let error = cache.put("b", Some(&json!("x".repeat(60)))).unwrap_err();
        assert!(error.contains("more than 100 bytes"), "{error}");
        cache.put("a", Some(&json!("y".repeat(60)))).unwrap();
        cache.put("a", None).unwrap();
        cache.put("b", Some(&json!("x".repeat(60)))).unwrap();
        drop(cache);
        let mut reopened = PluginCache::open_at(&path, 100).unwrap();
        assert!(reopened.put("c", Some(&json!("x".repeat(60)))).is_err());
    }
}
