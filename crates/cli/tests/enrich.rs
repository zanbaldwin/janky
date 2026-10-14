use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use janky_cli::cli::Checked;
use janky_cli::diagnostic::Diagnostics;
use janky_cli::generate::Kind;
use janky_cli::runner::{self, FakeHttp, Outcome};

fn data() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data")
}

fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let path = entry.unwrap().path();
        let target = to.join(path.file_name().unwrap());
        if path.is_dir() {
            copy(&path, &target);
        } else {
            std::fs::copy(&path, &target).unwrap();
        }
    }
}

fn project() -> (tempfile::TempDir, PathBuf) {
    let work = tempfile::tempdir().unwrap();
    let root = work.path().join("enrich-test");
    copy(&data().join("enrich-test"), &root);
    (work, root)
}

fn check(root: &Path) -> (Checked, Diagnostics) {
    let mut diagnostics = Diagnostics::default();
    let checked = janky_cli::cli::check(&root.join("deck.toml"), Default::default(), &mut diagnostics, &mut Vec::new());
    (checked.unwrap(), diagnostics)
}

fn run(checked: &Checked, root: &Path) -> Vec<(janky_cli::generate::Job, Outcome)> {
    let jobs = janky_cli::generate::pending(&checked.rendered.jobs).into_iter().cloned().collect();
    let limits = HashMap::from([("dict".to_string(), 1)]);
    runner::run(&checked.plugins, root, jobs, &limits, Arc::new(FakeHttp::default()), |_| {}, |_, _| true)
}

#[test]
fn enrich_fills_fields_from_cached_data_and_shows_warnings() {
    let (_work, root) = project();
    let (before, diagnostics) = check(&root);
    assert_eq!(diagnostics.to_json_lines(), "");
    assert!(before.rendered.jobs.iter().all(|job| job.kind == Kind::Data && !job.exists && job.file.is_empty()));
    assert_eq!(before.rendered.jobs.len(), 6);
    assert!(before.rendered.notes.iter().all(|note| note.fields["name"].is_empty() && note.fields["form"].is_empty()));
    assert!(before.rendered.media.is_empty());

    let results = run(&before, &root);
    assert!(results.iter().all(|(_, outcome)| matches!(outcome, Outcome::Stored { .. })), "{results:?}");
    let missing = results.iter().find(|(job, _)| job.input == "²⁹⁹Uue").unwrap();
    let stored: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(root.join(".janky/cache/data").join(format!("{}.json", missing.0.hash))).unwrap(),
    )
    .unwrap();
    assert_eq!(stored["value"], serde_json::json!({}));

    let (after, diagnostics) = check(&root);
    let expected = data().join("enrich-test/expected");
    assert_eq!(diagnostics.to_json_lines(), std::fs::read_to_string(expected.join("diagnostics.jsonl")).unwrap());
    assert_eq!(
        serde_json::to_string_pretty(&after.rendered).unwrap() + "\n",
        std::fs::read_to_string(expected.join("rendered.json")).unwrap()
    );
    assert!(after.rendered.media.is_empty());
}

#[test]
fn wrong_data_results_are_errors() {
    janky_cli::diagnostic::redact("data-secret-0123456789", "api_key");
    for (result, message) in [
        ("{ value = { k = 'x data-secret-0123456789' } }", "returned a secret value"),
        ("{ value = {}, warning = 'data-secret-0123456789' }", "returned a secret value"),
        ("{ value = { 'a', 'b' } }", "not a list"),
        ("{ value = 'x' }", "must return `value` as a table"),
        ("{ data = 'x', ext = 'txt' }", "return `value`"),
        ("{ value = {}, warning = 3 }", "must return `warning` as a string"),
        ("{ value = {}, warning = 'a\\0b' }", "without control characters"),
        ("{ value = {}, info = 'data-secret-0123456789' }", "returned a secret value"),
        ("{ value = {}, info = 3 }", "must return `info` as a string"),
        ("{ value = { x = string.rep('a', 1100000) } }", "longer than 1048576 bytes"),
    ] {
        let (_work, root) = project();
        let source = format!(
            "return {{ id = 'dict', api = 1, generates = 'data', generate = function() return {result} end }}\n"
        );
        std::fs::write(root.join("plugins/dict.lua"), source).unwrap();
        let (checked, _) = check(&root);
        let results = run(&checked, &root);
        let Outcome::Failed(failure) = &results[0].1 else {
            panic!("{result}: {results:?}")
        };
        assert!(failure.message.contains(message), "{result}: {}", failure.message);
    }
}

#[test]
fn plugins_say_what_they_generate() {
    for (table, message) in [
        ("{ id = 'dict', api = 1, generate = function() end }", "must say what it generates"),
        (
            "{ id = 'dict', api = 1, generates = 'files', generate = function() end }",
            "`generates` must be \\\"media\\\" or \\\"data\\\"",
        ),
        ("{ id = 'dict', api = 1, generates = 'data' }", "`generates` is allowed only with `generate`"),
        ("{ id = 'dict', api = 1, prepare = function() end }", "`prepare` is allowed only with `generate`"),
        (
            "{ id = 'dict', api = 1, generates = 'media', generate = function() end }",
            "`dict` generates media files, so call it with `media`",
        ),
    ] {
        let (_work, root) = project();
        std::fs::write(root.join("plugins/dict.lua"), format!("return {table}\n")).unwrap();
        let mut diagnostics = Diagnostics::default();
        janky_cli::cli::check(&root.join("deck.toml"), Default::default(), &mut diagnostics, &mut Vec::new());
        let lines = diagnostics.to_json_lines();
        assert!(lines.contains(message), "{table}: {lines}");
    }
}
