#![allow(dead_code)]

use std::path::{Path, PathBuf};

pub fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let path = entry.unwrap().path();
        if path.file_name() == Some(".janky".as_ref()) {
            continue;
        }
        let target = to.join(path.file_name().unwrap());
        if path.is_dir() {
            copy(&path, &target);
        } else {
            std::fs::copy(&path, &target).unwrap();
        }
    }
}

pub fn copy_example(to: &Path) {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    copy(&manifest.join("tests/data/example"), to);
    std::fs::copy(manifest.join("../../plugins/elevenlabs.lua"), to.join("plugins/elevenlabs.lua")).unwrap();
}

pub fn example() -> (tempfile::TempDir, PathBuf) {
    let work = tempfile::tempdir().unwrap();
    let dir = work.path().join("example");
    copy_example(&dir);
    seed(&dir);
    (work, dir)
}

pub fn seed(dir: &Path) {
    let mut diagnostics = janky_cli::diagnostic::Diagnostics::default();
    let checked = janky_cli::cli::check(&dir.join("deck.toml"), Default::default(), &mut diagnostics, &mut Vec::new());
    for job in &checked.unwrap().rendered.jobs {
        if job.kind == janky_cli::generate::Kind::Data {
            let data = janky_cli::cache::Data {
                instance: job.instance.clone(),
                value: fake_data(&job.input),
                warning: None,
                info: None,
                cost: None,
            };
            janky_cli::cache::store_data(dir, &job.hash, &data).unwrap();
            continue;
        }
        let ext = job.ext.clone().unwrap_or_else(|| "ogg".into());
        let meta = janky_cli::cache::Meta {
            ext,
            instance: job.instance.clone(),
            cost: None,
        };
        janky_cli::cache::store(dir, &job.hash, fake_audio(&job.input_text()).as_bytes(), &meta).unwrap();
    }
}

pub fn fake_audio(input: &str) -> String {
    format!("fake audio for {input}")
}

pub fn fake_data(input: &serde_json::Value) -> serde_json::Value {
    let symbol = input.as_str().unwrap();
    serde_json::json!({
        "number": 1,
        "mass": format!("{symbol}.0"),
        "config": format!("[He]2s1 {symbol}"),
        "state": "Solid",
        "group_block": "Metal ⚛️",
        "discovered": "Ancient",
        "colour": format!("<b>{symbol}</b>"),
    })
}
