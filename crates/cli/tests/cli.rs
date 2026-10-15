use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

mod common;

fn data() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data")
}

fn janky(project: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_janky"))
        .arg("--deck")
        .arg(project)
        .args(args)
        .env("NO_COLOR", "1")
        .output()
        .unwrap()
}

fn seed_media(deck: &Path) {
    common::seed(deck.parent().unwrap());
}

fn copy(from: &Path, to: &Path) {
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

#[test]
fn check_build_and_schema_on_the_example() {
    let work = tempfile::tempdir().unwrap();
    let project = work.path().join("example");
    common::copy_example(&project);
    seed_media(&project.join("deck.toml"));
    let deck = project.join("deck.toml");

    let tree = |dir: &Path| {
        let mut names: Vec<String> = Vec::new();
        let mut stack = vec![dir.to_path_buf()];
        while let Some(path) = stack.pop() {
            for entry in std::fs::read_dir(&path).unwrap() {
                let path = entry.unwrap().path();
                names.push(path.strip_prefix(dir).unwrap().display().to_string());
                if path.is_dir() {
                    stack.push(path);
                }
            }
        }
        names.sort();
        names
    };
    let before = tree(&project);
    let check = janky(&deck, &["check"]);
    assert!(check.status.success());
    assert_eq!(String::from_utf8_lossy(&check.stderr), "7 notes are valid\n");
    assert_eq!(tree(&project), before);
    let lock_before = std::fs::read_to_string(project.join("deck.lock")).unwrap();

    let build = janky(&deck, &["build"]);
    assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));
    let stderr = String::from_utf8_lossy(&build.stderr);
    assert!(stderr.contains("periodic.apkg: 7 notes, 6 media files\n"), "{stderr}");
    assert!(stderr.contains("turn FSRS on"));
    let apkg = project.join(".janky/build/periodic.apkg");
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(std::fs::metadata(&apkg).unwrap().permissions().mode() & 0o777, 0o644);
    let lock = std::fs::read_to_string(project.join("deck.lock")).unwrap();
    assert!(lock.starts_with(lock_before.lines().next().unwrap()));
    assert!(lock.contains("\n[notes]\nalkali-metals = { hash = "));

    let imported = janky_anki::import(
        &apkg,
        &work.path().join("collection"),
        janky_anki::ImportOptions {
            update_if_newer: true,
            merge_notetypes: true,
            with_deck_configs: true,
        },
    )
    .unwrap();
    assert_eq!((imported.new, imported.notes.len()), (7, 7));

    let quiet = janky(&deck, &["build", "--quiet"]);
    assert!(quiet.status.success());
    assert!(quiet.stderr.is_empty());
    assert_eq!(std::fs::read_to_string(project.join("deck.lock")).unwrap(), lock);
    let again = janky_anki::import(
        &apkg,
        &work.path().join("collection"),
        janky_anki::ImportOptions {
            update_if_newer: true,
            merge_notetypes: true,
            with_deck_configs: true,
        },
    )
    .unwrap();
    assert_eq!((again.new, again.updated), (0, 0));

    let schema = janky(&deck, &["schema"]);
    assert!(schema.status.success());
    for name in ["deck", "notetype", "lock", "content"] {
        let text = std::fs::read_to_string(project.join(format!(".janky/schema/{name}.schema.json"))).unwrap();
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert!(value.is_object(), "{name}");
    }
    assert!(String::from_utf8_lossy(&schema.stderr).contains("include = [\"content/groups/*.toml\"]"));
}

#[test]
fn errors_fail_with_json_lines_and_write_nothing() {
    let case = data().join("errors/s1-json");
    let output = janky(&case.join("deck.toml"), &["build", "--json", "--quiet"]);
    assert_eq!(output.status.code(), Some(1));
    let expected = std::fs::read_to_string(case.join("expected.jsonl")).unwrap();
    assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
    assert!(output.stderr.is_empty());
    assert!(!case.join(".janky").exists());
    assert!(!case.join("deck.lock").exists());
}

#[test]
fn text_diagnostics_go_to_stderr() {
    let output = janky(&data().join("errors/toml-syntax/deck.toml"), &["check"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("toml::syntax"));
    assert!(stderr.contains("[deck.toml:3:10]"));
}

#[test]
fn build_never_writes_through_symlinks() {
    let work = tempfile::tempdir().unwrap();
    let project = work.path().join("example");
    common::copy_example(&project);
    seed_media(&project.join("deck.toml"));
    let deck = project.join("deck.toml");
    let outside = work.path().join("outside.txt");
    std::fs::write(&outside, "keep").unwrap();

    std::os::unix::fs::symlink(&outside, project.join("deck.lock.tmp")).unwrap();
    let output = janky(&deck, &["build"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("build::lock"));
    assert_eq!(std::fs::read_to_string(&outside).unwrap(), "keep");
    std::fs::remove_file(project.join("deck.lock.tmp")).unwrap();

    let elsewhere = work.path().join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    std::fs::remove_dir_all(project.join(".janky")).unwrap();
    std::os::unix::fs::symlink(&elsewhere, project.join(".janky")).unwrap();
    for command in ["build", "schema"] {
        let output = janky(&deck, &[command, "--allow-missing"]);
        assert_eq!(output.status.code(), Some(1), "{command}");
        assert!(String::from_utf8_lossy(&output.stderr).contains("outside the project directory"), "{command}");
    }
    assert_eq!(std::fs::read_dir(&elsewhere).unwrap().count(), 0);
    std::fs::remove_file(project.join(".janky")).unwrap();

    std::fs::create_dir_all(project.join(".janky/build")).unwrap();
    std::os::unix::fs::symlink(&outside, project.join(".janky/build/periodic.apkg")).unwrap();
    let output = janky(&deck, &["build", "--allow-missing"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("symbolic link"));
    assert_eq!(std::fs::read_to_string(&outside).unwrap(), "keep");
}

#[test]
fn plan_shows_an_edit_and_build_applies_it() {
    let work = tempfile::tempdir().unwrap();
    let project = work.path().join("example");
    common::copy_example(&project);
    seed_media(&project.join("deck.toml"));
    let deck = project.join("deck.toml");

    let unchanged = janky(&deck, &["plan"]);
    assert!(unchanged.status.success());
    assert_eq!(
        String::from_utf8_lossy(&unchanged.stdout),
        "Notes: 0 added, 0 changed, 0 removed, 0 moved, 7 unchanged\nNo changes.\n"
    );

    let early = project.join("content/periods/early.yaml");
    let text = std::fs::read_to_string(&early).unwrap().replace("floats on oil", "floats on water");
    std::fs::write(&early, text).unwrap();
    seed_media(&deck);
    let lock = std::fs::read_to_string(project.join("deck.lock")).unwrap();
    let plan = janky(&deck, &["plan", "--json"]);
    assert!(plan.status.success());
    let value: serde_json::Value = serde_json::from_slice(&plan.stdout).unwrap();
    assert_eq!(
        value["plan"]["changes"],
        serde_json::json!([{ "kind": "note-changed", "id": "li", "deck": "elements" }])
    );
    assert_eq!(std::fs::read_to_string(project.join("deck.lock")).unwrap(), lock);

    let build = janky(&deck, &["build"]);
    assert!(build.status.success());
    assert!(
        String::from_utf8_lossy(&build.stderr)
            .starts_with("Notes: 0 added, 1 changed, 0 removed, 0 moved, 6 unchanged\n~ note li")
    );
    let after = janky(&deck, &["plan"]);
    assert!(String::from_utf8_lossy(&after.stdout).ends_with("No changes.\n"));
}

fn git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "init.defaultBranch=main",
        ])
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

#[test]
fn a_git_note_type_builds_from_the_pinned_commit() {
    let work = tempfile::tempdir().unwrap();
    let remote = work.path().join("remote");
    std::fs::create_dir_all(remote.join("element/cards")).unwrap();
    std::fs::write(
        remote.join("element/notetype.toml"),
        "name = \"Remote\"\n\n[schema]\nsymbol = { type = \"string\", required = true }\n\n[fields.symbol]\nname = \"Symbol\"\n\n\
         [cards.recognition]\nname = \"Recognition\"\nfront = \"cards/front.html\"\nback = \"cards/back.html\"\n",
    )
    .unwrap();
    std::fs::write(remote.join("element/cards/front.html"), "{{Symbol}}\n").unwrap();
    std::fs::write(remote.join("element/cards/back.html"), "{{FrontSide}}\n").unwrap();
    git(&remote, &["init", "-q"]);
    git(&remote, &["add", "."]);
    git(&remote, &["commit", "-qm", "one"]);
    git(&remote, &["tag", "v1"]);
    let commit = git(&remote, &["rev-parse", "HEAD"]);

    let url = "https://example.invalid/notetypes.git";
    let cache_home = work.path().join("cache");
    let repo = janky_cli::source::repo_dir(&cache_home.join("janky/sources"), url);
    std::fs::create_dir_all(repo.parent().unwrap()).unwrap();
    git(
        work.path(),
        &[
            "clone",
            "-q",
            "--bare",
            "--ref-format=files",
            remote.to_str().unwrap(),
            repo.to_str().unwrap(),
        ],
    );
    git(&repo, &["remote", "set-url", "origin", url]);

    let project = work.path().join("project");
    std::fs::create_dir_all(project.join("content")).unwrap();
    std::fs::write(
        project.join("deck.toml"),
        format!("[deck]\nname = \"Remote ⚛️\"\ncontent = [\"content/*.yaml\"]\n\n[notetypes.remote]\ngit = \"{url}\"\ntag = \"v1\"\npath = \"element\"\ndefault = true\n"),
    )
    .unwrap();
    std::fs::write(project.join("content/words.yaml"), "entries:\n  - id: 'atom'\n    symbol: '⚛️'\n").unwrap();
    std::fs::write(
        project.join("deck.lock"),
        format!(
            "version = 1\nnamespace = \"00112233445566778899aabbccddeeff\"\n\n[notetypes.remote]\nid = 1000000000001\n\
             source = {{ git = \"{url}\", tag = \"v1\", commit = \"{commit}\" }}\n"
        ),
    )
    .unwrap();

    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_janky"))
            .arg("--deck")
            .arg(project.join("deck.toml"))
            .args(args)
            .env("NO_COLOR", "1")
            .env("XDG_CACHE_HOME", &cache_home)
            .output()
            .unwrap()
    };
    let build = run(&["build"]);
    assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));
    let lock = std::fs::read_to_string(project.join("deck.lock")).unwrap();
    assert!(lock.contains(&format!("source = {{ git = \"{url}\", tag = \"v1\", commit = \"{commit}\" }}")));
    assert!(lock.contains("name = \"Remote\""));

    let deck = std::fs::read_to_string(project.join("deck.toml")).unwrap().replace("tag = \"v1\"", "tag = \"v2\"");
    std::fs::write(project.join("deck.toml"), deck).unwrap();
    let check = run(&["check", "--json"]);
    assert_eq!(check.status.code(), Some(1));
    let diagnostic: serde_json::Value =
        serde_json::from_slice(check.stdout.split(|byte| *byte == b'\n').next().unwrap()).unwrap();
    assert_eq!(diagnostic["code"], "source::git");
    assert!(diagnostic["message"].as_str().unwrap().starts_with("cannot fetch"));
}

#[test]
fn a_git_url_without_a_pinned_commit_warns() {
    let work = tempfile::tempdir().unwrap();
    let project = work.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(
        project.join("deck.toml"),
        "[deck]\nname = \"Remote ⚛️\"\n\n[notetypes.remote]\ngit = \"git://127.0.0.1:1/notetypes.git\"\ntag = \"v1\"\n",
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_janky"))
        .arg("--deck")
        .arg(project.join("deck.toml"))
        .args(["check", "--json"])
        .env("XDG_CACHE_HOME", work.path().join("cache"))
        .output()
        .unwrap();
    let codes: Vec<(String, String)> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .map(|value| (value["code"].as_str().unwrap().to_string(), value["severity"].as_str().unwrap().to_string()))
        .collect();
    assert_eq!(
        codes,
        [
            ("source::insecure".to_string(), "warning".to_string()),
            ("source::git".to_string(), "error".to_string())
        ]
    );
}

#[test]
fn a_url_note_type_builds_from_the_archive() {
    let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default()));
    for (name, text) in [
        (
            "pack/notetype.toml",
            "name = \"Archive\"\n\n[schema]\nsymbol = { type = \"string\", required = true }\n\n[fields.symbol]\nname = \"Symbol\"\n\n\
             [cards.recognition]\nname = \"Recognition\"\nfront = \"front.html\"\nback = \"back.html\"\n",
        ),
        ("pack/front.html", "{{Symbol}}\n"),
        ("pack/back.html", "{{FrontSide}}\n"),
    ] {
        let mut header = tar::Header::new_gnu();
        header.set_size(text.len() as u64);
        header.set_mode(0o644);
        builder.append_data(&mut header, name, text.as_bytes()).unwrap();
    }
    let archive = builder.into_inner().unwrap().finish().unwrap();
    let sha256: String =
        <sha2::Sha256 as sha2::Digest>::digest(&archive).iter().map(|byte| format!("{byte:02x}")).collect();

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/pack.tar.gz", listener.local_addr().unwrap());
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = Vec::new();
        let mut buffer = [0u8; 1024];
        while !request.windows(4).any(|window| window == b"\r\n\r\n") {
            let read = stream.read(&mut buffer).unwrap();
            request.extend_from_slice(&buffer[..read]);
        }
        let head = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", archive.len());
        stream.write_all(head.as_bytes()).unwrap();
        stream.write_all(&archive).unwrap();
    });

    let work = tempfile::tempdir().unwrap();
    let project = work.path().join("project");
    std::fs::create_dir_all(project.join("content")).unwrap();
    std::fs::write(
        project.join("deck.toml"),
        format!(
            "[deck]\nname = \"Archive 🧪\"\ncontent = [\"content/*.yaml\"]\n\n[notetypes.packed]\nurl = \"{url}\"\nsha256 = \"{sha256}\"\npath = \"pack\"\ndefault = true\n"
        ),
    )
    .unwrap();
    std::fs::write(project.join("content/words.yaml"), "entries:\n  - id: 'atom'\n    symbol: '⚛️'\n").unwrap();
    let build = Command::new(env!("CARGO_BIN_EXE_janky"))
        .arg("--deck")
        .arg(project.join("deck.toml"))
        .arg("build")
        .env("XDG_CACHE_HOME", work.path().join("cache"))
        .output()
        .unwrap();
    server.join().unwrap();
    assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));
    let lock = std::fs::read_to_string(project.join("deck.lock")).unwrap();
    assert!(lock.contains(&format!("source = {{ url = \"{url}\", sha256 = \"{sha256}\" }}")));
    assert!(lock.contains("name = \"Archive\""));
}

#[test]
fn update_moves_a_branch_source_and_offline_uses_the_cache() {
    let work = tempfile::tempdir().unwrap();
    let remote = work.path().join("remote");
    std::fs::create_dir_all(remote.join("cards")).unwrap();
    let notetype = |name: &str| {
        format!(
            "name = \"{name}\"\n\n[fields.symbol]\nname = \"Symbol\"\n\n[cards.recognition]\nname = \"Recognition\"\n\
             front = \"cards/front.html\"\nback = \"cards/front.html\"\n"
        )
    };
    std::fs::write(remote.join("notetype.toml"), notetype("One")).unwrap();
    std::fs::write(remote.join("cards/front.html"), "{{Symbol}}\n").unwrap();
    git(&remote, &["init", "-q"]);
    git(&remote, &["add", "."]);
    git(&remote, &["commit", "-qm", "one"]);
    let first = git(&remote, &["rev-parse", "HEAD"]);

    let url = "https://example.invalid/branch.git";
    let cache_home = work.path().join("cache");
    let repo = janky_cli::source::repo_dir(&cache_home.join("janky/sources"), url);
    std::fs::create_dir_all(repo.parent().unwrap()).unwrap();
    git(
        work.path(),
        &[
            "clone",
            "-q",
            "--bare",
            "--ref-format=files",
            remote.to_str().unwrap(),
            repo.to_str().unwrap(),
        ],
    );

    let project = work.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(
        project.join("deck.toml"),
        format!("[deck]\nname = \"Update 👩‍🔬\"\n\n[notetypes.remote]\ngit = \"{url}\"\nbranch = \"main\"\n"),
    )
    .unwrap();
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_janky"))
            .arg("--deck")
            .arg(project.join("deck.toml"))
            .args(args)
            .env("NO_COLOR", "1")
            .env("XDG_CACHE_HOME", &cache_home)
            .output()
            .unwrap()
    };
    let lock = || std::fs::read_to_string(project.join("deck.lock")).unwrap();
    let build = run(&["build", "--quiet"]);
    assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));
    assert!(lock().contains(&format!("commit = \"{first}\"")));

    std::fs::write(remote.join("notetype.toml"), notetype("Two")).unwrap();
    git(&remote, &["commit", "-qam", "two"]);
    let second = git(&remote, &["rev-parse", "HEAD"]);
    assert!(run(&["build", "--quiet"]).status.success());
    assert!(lock().contains(&format!("commit = \"{first}\"")));

    let dry = run(&["update", "--offline"]);
    assert!(dry.status.success());
    assert!(String::from_utf8_lossy(&dry.stderr).ends_with(&format!("remote: unchanged at {}\n", &first[..12])));
    git(&repo, &["fetch", "-q", remote.to_str().unwrap(), "+refs/heads/*:refs/heads/*"]);
    let dry = run(&["update", "--offline"]);
    assert!(String::from_utf8_lossy(&dry.stderr).starts_with("Dry run with --offline"));
    assert!(String::from_utf8_lossy(&dry.stderr).ends_with(&format!(
        "remote: {} -> {}\n",
        &first[..12],
        &second[..12]
    )));
    assert!(lock().contains(&format!("commit = \"{first}\"")));

    let update = run(&["update"]);
    assert!(update.status.success(), "{}", String::from_utf8_lossy(&update.stderr));
    assert_eq!(String::from_utf8_lossy(&update.stderr), format!("remote: {} -> {}\n", &first[..12], &second[..12]));
    assert!(lock().contains(&format!("commit = \"{second}\"")));

    std::fs::remove_dir_all(&remote).unwrap();
    let offline = run(&["build", "--offline"]);
    assert!(offline.status.success(), "{}", String::from_utf8_lossy(&offline.stderr));
    assert!(lock().contains("name = \"Two\""));
}

#[test]
fn plugin_output_cannot_send_terminal_escapes() {
    let work = tempfile::tempdir().unwrap();
    let project = work.path().join("plugin-test");
    copy(&data().join("plugin-test"), &project);
    let script = project.join("plugins/text.lua");
    let text = std::fs::read_to_string(&script).unwrap();
    std::fs::write(&script, format!("print(\"\\27[2Jcleared\\nerror: fake\")\n{text}")).unwrap();
    let output = janky(&project.join("deck.toml"), &["check"]);
    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.contains('\u{1b}'), "{stderr:?}");
    assert!(stderr.starts_with("plugin: \\u{1b}[2Jcleared\nplugin: error: fake\n"), "{stderr:?}");
}

#[test]
fn schema_includes_the_plugin_variables() {
    let work = tempfile::tempdir().unwrap();
    let project = work.path().join("plugin-test");
    copy(&data().join("plugin-test"), &project);
    let output = janky(&project.join("deck.toml"), &["schema"]);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let text = std::fs::read_to_string(project.join(".janky/schema/deck.schema.json")).unwrap();
    let schema: serde_json::Value = serde_json::from_str(&text).unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();
    let deck = std::fs::read_to_string(project.join("deck.toml")).unwrap();
    assert!(validator.is_valid(&toml::from_str::<serde_json::Value>(&deck).unwrap()));
    let wrong = deck.replace("ending = \"!\"", "ending = 1");
    assert!(!validator.is_valid(&toml::from_str::<serde_json::Value>(&wrong).unwrap()));
    let missing = deck.replace("variables = { ending = \"!\" }\n", "");
    assert!(!validator.is_valid(&toml::from_str::<serde_json::Value>(&missing).unwrap()));
}

#[test]
fn a_plugin_edit_shows_the_notes_it_changes() {
    let work = tempfile::tempdir().unwrap();
    let project = work.path().join("example");
    common::copy_example(&project);
    seed_media(&project.join("deck.toml"));
    let deck = project.join("deck.toml");
    let script = project.join("notetypes/element/atoms.lua");
    let text = std::fs::read_to_string(&script).unwrap();
    std::fs::write(&script, text.replace("<sup>", "<small>").replace("</sup>", "</small>")).unwrap();

    let plan = janky(&deck, &["plan"]);
    assert!(plan.status.success());
    assert_eq!(
        String::from_utf8_lossy(&plan.stdout),
        "Notes: 0 added, 4 changed, 0 removed, 0 moved, 3 unchanged\n~ note li in Periodic Table::0 Elements\n\
         ~ note c in Periodic Table::0 Elements\n~ note fe in Periodic Table::0 Elements\n~ note au in Periodic Table::0 Elements\n"
    );
}

#[test]
fn two_builds_with_the_same_input_give_the_same_fields() {
    let work = tempfile::tempdir().unwrap();
    let project = work.path().join("example");
    common::copy_example(&project);
    seed_media(&project.join("deck.toml"));
    let deck = project.join("deck.toml");
    let lock = std::fs::read_to_string(project.join("deck.lock")).unwrap();
    for _ in 0..2 {
        let build = janky(&deck, &["build"]);
        assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));
        assert!(String::from_utf8_lossy(&build.stderr).starts_with("Notes: 0 added, 0 changed, 0 removed, 0 moved"));
        assert_eq!(std::fs::read_to_string(project.join("deck.lock")).unwrap(), lock);
    }
    let first = janky(&deck, &["plan", "--json"]);
    let second = janky(&deck, &["plan", "--json"]);
    assert_eq!(first.stdout, second.stdout);
}

#[test]
fn check_works_in_the_project_directory_without_project() {
    let work = tempfile::tempdir().unwrap();
    let project = work.path().join("plugin-test");
    copy(&data().join("plugin-test"), &project);
    let output = Command::new(env!("CARGO_BIN_EXE_janky"))
        .arg("check")
        .current_dir(&project)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
}

#[test]
fn schema_writes_the_fixed_schemas_before_plugins_run() {
    let work = tempfile::tempdir().unwrap();
    let project = work.path().join("plugin-test");
    copy(&data().join("plugin-test"), &project);
    std::fs::write(project.join("plugins/text.lua"), "error('broken')").unwrap();
    let output = janky(&project.join("deck.toml"), &["schema"]);
    assert!(!output.status.success());
    for name in ["deck", "notetype", "lock", "content"] {
        assert!(project.join(format!(".janky/schema/{name}.schema.json")).is_file(), "{name}");
    }
}

#[test]
fn a_device_as_plugin_is_not_a_regular_file() {
    let work = tempfile::tempdir().unwrap();
    let project = work.path().join("plugin-test");
    copy(&data().join("plugin-test"), &project);
    let script = project.join("plugins/text.lua");
    std::fs::remove_file(&script).unwrap();
    std::os::unix::fs::symlink("/dev/null", &script).unwrap();
    let output = janky(&project.join("deck.toml"), &["check"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("`plugins/text.lua` is not a regular file"));
}

#[test]
fn missing_generated_media_is_a_warning_in_check_and_an_error_in_build() {
    let work = tempfile::tempdir().unwrap();
    let project = work.path().join("generate-test");
    copy(&data().join("generate-test"), &project);
    let deck = project.join("deck.toml");
    let check = janky(&deck, &["check"]);
    assert!(check.status.success());
    assert!(String::from_utf8_lossy(&check.stderr).contains("4 generated media files are missing"));
    let build = janky(&deck, &["build"]);
    assert!(!build.status.success());
    assert!(String::from_utf8_lossy(&build.stderr).contains("or build with `--allow-missing`"));
    assert!(!project.join(".janky/build/deck.apkg").exists());
    let allowed = janky(&deck, &["build", "--allow-missing"]);
    assert!(allowed.status.success(), "{}", String::from_utf8_lossy(&allowed.stderr));
    assert!(project.join(".janky/build/deck.apkg").exists());
}

#[test]
fn cached_generated_media_goes_into_the_package() {
    let work = tempfile::tempdir().unwrap();
    let project = work.path().join("generate-test");
    copy(&data().join("generate-test"), &project);
    let deck = project.join("deck.toml");
    let plan = janky(&deck, &["plan"]);
    let text = String::from_utf8_lossy(&plan.stdout);
    assert!(text.contains("Generator jobs: 4 to run (2 characters)\n+ media voice: ☿\n"), "{text}");

    let mut diagnostics = janky_cli::diagnostic::Diagnostics::default();
    let checked = janky_cli::cli::check(&deck, Default::default(), &mut diagnostics, &mut Vec::new()).unwrap();
    for job in &checked.rendered.jobs {
        let ext = job.ext.clone().unwrap_or_else(|| "svg".into());
        let meta = janky_cli::cache::Meta {
            ext,
            instance: job.instance.clone(),
            cost: None,
        };
        janky_cli::cache::store(&project, &job.hash, job.input_text().as_bytes(), &meta).unwrap();
    }

    let check = janky(&deck, &["check"]);
    assert!(!String::from_utf8_lossy(&check.stderr).contains("missing"));
    let build = janky(&deck, &["build"]);
    let stderr = String::from_utf8_lossy(&build.stderr);
    assert!(build.status.success(), "{stderr}");
    assert!(stderr.contains("3 notes, 4 media files"), "{stderr}");
    let lock = std::fs::read_to_string(project.join("deck.lock")).unwrap();
    let plain = checked.rendered.jobs.iter().find(|job| job.instance == "plain").unwrap();
    assert!(lock.contains(&format!("\"{}.svg\"", plain.hash)), "{lock}");
    let after = janky(&deck, &["plan"]);
    assert!(String::from_utf8_lossy(&after.stdout).ends_with("No changes.\n"));
}

#[test]
fn missing_enrichment_warns_in_check_and_plan_and_stops_build() {
    let work = tempfile::tempdir().unwrap();
    let project = work.path().join("enrich-test");
    copy(&data().join("enrich-test"), &project);
    let deck = project.join("deck.toml");

    let check = janky(&deck, &["check"]);
    let stderr = String::from_utf8_lossy(&check.stderr);
    assert!(check.status.success(), "{stderr}");
    assert!(stderr.contains("6 enrichment results are missing"), "{stderr}");

    let plan = janky(&deck, &["plan"]);
    let stdout = String::from_utf8_lossy(&plan.stdout);
    assert!(
        stdout.contains("+ enrich dict: ⁵⁶Fe\n")
            && stdout.contains("+ enrich dict: {\"isotope\":\"⁵⁶Fe\",\"phase\":\"solid\"}\n"),
        "{stdout}"
    );

    let build = janky(&deck, &["build"]);
    assert!(!build.status.success());
    assert!(String::from_utf8_lossy(&build.stderr).contains("enrich::missing"));
    let allowed = janky(&deck, &["build", "--allow-missing"]);
    assert!(allowed.status.success(), "{}", String::from_utf8_lossy(&allowed.stderr));
}

#[test]
fn generate_and_prune_wait_for_the_project_lock() {
    let work = tempfile::tempdir().unwrap();
    let deck = generate_project(work.path());
    let project = deck.parent().unwrap();
    std::fs::create_dir_all(project.join(".janky")).unwrap();
    let held = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(project.join(".janky/generate.lock"))
        .unwrap();
    held.try_lock().unwrap();
    for command in [["generate", "--yes"], ["prune", "--yes"]] {
        let output = janky(&deck, &command);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success(), "{stderr}");
        assert!(stderr.contains("generate::locked"), "{stderr}");
    }
    assert!(!project.join(".janky/cache/media").exists());
    drop(held);
    let output = janky(&deck, &["generate", "--yes"]);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
}

fn generate_project(work: &Path) -> PathBuf {
    let project = work.join("generate-test");
    copy(&data().join("generate-test"), &project);
    project.join("deck.toml")
}

#[test]
fn media_generate_fills_the_cache_and_prune_cleans_it() {
    let work = tempfile::tempdir().unwrap();
    let deck = generate_project(work.path());
    let project = deck.parent().unwrap();

    let refused = janky(&deck, &["generate"]);
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("--yes"));
    assert!(!project.join(".janky/cache/media").exists());

    let generated = janky(&deck, &["generate", "--yes"]);
    let stderr = String::from_utf8_lossy(&generated.stderr);
    assert!(generated.status.success(), "{stderr}");
    assert!(stderr.contains("Generator jobs: 4 to run"), "{stderr}");
    assert!(stderr.contains("plain: 2 jobs\n  network: none\n"), "{stderr}");
    assert!(stderr.contains("[4/4] "), "{stderr}");
    assert!(stderr.contains("Generated 4 results: 0 refused, 0 failed, 0 not started"), "{stderr}");
    let check = janky(&deck, &["check"]);
    assert!(!String::from_utf8_lossy(&check.stderr).contains("missing"));
    let again = janky(&deck, &["generate", "--yes"]);
    assert!(String::from_utf8_lossy(&again.stderr).contains("All generator results are in the cache."));

    let cache = project.join(".janky/cache/media");
    std::fs::write(cache.join("0123abcd.ogg"), b"old").unwrap();
    std::fs::write(cache.join("0123abcd.json"), b"{}").unwrap();
    std::fs::write(cache.join(".0123abcd.ogg.tmp-99"), b"half").unwrap();
    let old = std::time::SystemTime::now() - std::time::Duration::from_secs(7200);
    std::fs::File::options()
        .write(true)
        .open(cache.join(".0123abcd.ogg.tmp-99"))
        .unwrap()
        .set_modified(old)
        .unwrap();
    std::fs::write(cache.join(".fresh.ogg.tmp-1"), b"busy").unwrap();
    std::fs::write(cache.join("notes.txt"), b"mine").unwrap();
    let pruned = janky(&deck, &["prune", "--yes"]);
    let stderr = String::from_utf8_lossy(&pruned.stderr);
    assert!(pruned.status.success(), "{stderr}");
    assert!(stderr.contains("Removed 3 files (9 bytes)."), "{stderr}");
    assert!(cache.join("notes.txt").exists() && cache.join(".fresh.ogg.tmp-1").exists());
    assert_eq!(std::fs::read_dir(&cache).unwrap().count(), 10);
    let check = janky(&deck, &["check"]);
    assert!(!String::from_utf8_lossy(&check.stderr).contains("missing"));
}

#[test]
fn generate_runs_data_jobs_and_prune_waits_for_them() {
    let work = tempfile::tempdir().unwrap();
    let project = work.path().join("enrich-test");
    copy(&data().join("enrich-test"), &project);
    let deck = project.join("deck.toml");
    let plugin = project.join("plugins/dict.lua");
    let source = std::fs::read_to_string(&plugin).unwrap();
    std::fs::write(&plugin, source.replace("generate = generate }", "generate = generate, prepare = function() end }"))
        .unwrap();
    let data = project.join(".janky/cache/data");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::write(data.join("0123abcd.json"), b"{}").unwrap();

    let refused = janky(&deck, &["prune", "--yes"]);
    let stderr = String::from_utf8_lossy(&refused.stderr);
    assert!(!refused.status.success(), "{stderr}");
    assert!(stderr.contains("prune::pending") && stderr.contains("6 generated results are missing"), "{stderr}");
    assert!(stderr.contains("Pending: dict `") && stderr.contains(", 1 more"), "{stderr}");
    assert!(data.join("0123abcd.json").exists());
    let allowed = janky(&deck, &["prune", "--yes", "--allow-missing"]);
    let stderr = String::from_utf8_lossy(&allowed.stderr);
    assert!(allowed.status.success(), "{stderr}");
    assert!(stderr.contains("Removed 1 files (2 bytes)."), "{stderr}");
    assert!(!data.join("0123abcd.json").exists());

    let unconfirmed = janky(&deck, &["generate"]);
    assert!(!unconfirmed.status.success());
    assert!(String::from_utf8_lossy(&unconfirmed.stderr).contains("--yes"));
    assert_eq!(std::fs::read_dir(&data).unwrap().count(), 0);

    let generated = janky(&deck, &["generate", "--yes"]);
    let stderr = String::from_utf8_lossy(&generated.stderr);
    assert!(generated.status.success(), "{stderr}");
    assert!(stderr.contains("Generator jobs: 6 to run\n+ enrich dict: "), "{stderr}");
    assert!(stderr.contains("dict: 6 jobs\n  network: none\n"), "{stderr}");
    assert!(stderr.contains("dict: running `prepare`\n"), "{stderr}");
    assert!(stderr.contains("[6/6] dict: "), "{stderr}");
    assert!(stderr.contains("Generated 6 results: 0 refused, 0 failed, 0 not started"), "{stderr}");
    assert_eq!(std::fs::read_dir(&data).unwrap().count(), 6);
    let check = janky(&deck, &["check"]);
    assert!(!String::from_utf8_lossy(&check.stderr).contains("missing"));
    let again = janky(&deck, &["generate", "--yes"]);
    assert!(String::from_utf8_lossy(&again.stderr).contains("All generator results are in the cache."));

    std::fs::write(data.join("4567cdef.json"), b"{}").unwrap();
    let pruned = janky(&deck, &["prune", "--yes"]);
    let stderr = String::from_utf8_lossy(&pruned.stderr);
    assert!(pruned.status.success(), "{stderr}");
    assert!(stderr.contains("Removed 1 files (2 bytes)."), "{stderr}");
    assert_eq!(std::fs::read_dir(&data).unwrap().count(), 6);
}

#[test]
fn a_home_relative_secret_file_needs_home() {
    let work = tempfile::tempdir().unwrap();
    let deck = generate_project(work.path());
    let project = deck.parent().unwrap();
    std::fs::write(
        project.join("plugins/voice.lua"),
        "local function generate(job) return nil, 'unused' end\n\
         return { id = 'voice', api = 1, generates = 'media', secrets = { 'api_key' }, generate = generate, \
         variables = { speed = { type = 'int' }, voice = { type = 'string' } } }\n",
    )
    .unwrap();
    let text = std::fs::read_to_string(&deck).unwrap();
    std::fs::write(&deck, format!("{text}\n[plugins.voice.secrets]\napi_key = {{ file = \"~/key.txt\" }}\n")).unwrap();
    let home = work.path().join("home");
    std::fs::create_dir(&home).unwrap();
    std::fs::write(home.join("key.txt"), "home-key-0123456789\n").unwrap();
    let generate = |home: Option<&Path>| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_janky"));
        command.arg("--deck").arg(&deck).arg("generate").env("NO_COLOR", "1");
        match home {
            Some(home) => command.env("HOME", home),
            None => command.env_remove("HOME"),
        };
        command.output().unwrap()
    };

    let unset = generate(None);
    let stderr = String::from_utf8_lossy(&unset.stderr);
    assert!(!unset.status.success());
    assert!(stderr.contains("`HOME` is not set, so `~/key.txt` cannot be found"), "{stderr}");

    let set = generate(Some(&home));
    let stderr = String::from_utf8_lossy(&set.stderr);
    assert!(!stderr.contains("secret::missing"), "{stderr}");
    assert!(stderr.contains("--yes"), "{stderr}");
}

#[test]
fn media_generate_shows_hosts_and_secret_sources_and_stops_when_refused() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = Vec::new();
        let mut buffer = [0u8; 1024];
        while !request.windows(4).any(|window| window == b"\r\n\r\n") {
            let read = stream.read(&mut buffer).unwrap();
            request.extend_from_slice(&buffer[..read]);
        }
        stream.write_all(b"HTTP/1.1 401 X\r\nContent-Length: 5\r\nConnection: close\r\n\r\nquota").unwrap();
    });
    let work = tempfile::tempdir().unwrap();
    let deck = generate_project(work.path());
    let project = deck.parent().unwrap();
    std::fs::write(
        project.join("plugins/voice.lua"),
        format!(
            "local function generate(job)\n\
               local r = janky.http.request {{ url = '{base}/tts', headers = {{ ['xi-api-key'] = janky.secret('api_key') }} }}\n\
               if r.status ~= 200 then return nil, 'HTTP ' .. r.status .. ' ' .. r.body end\n\
               return {{ data = r.body, ext = 'ogg' }}\n\
             end\n\
             return {{ id = 'voice', api = 1, generates = 'media', secrets = {{ 'api_key' }}, generate = generate, \
             variables = {{ speed = {{ type = 'int' }}, voice = {{ type = 'string' }} }} }}\n"
        ),
    )
    .unwrap();
    let text = std::fs::read_to_string(&deck).unwrap();
    std::fs::write(
        &deck,
        format!("{text}\n[plugins.voice.http]\nallowed_hosts = [\"127.0.0.1\"]\n\n[plugins.voice.secrets]\napi_key = {{ file = \"key.txt\" }}\n"),
    )
    .unwrap();

    let missing = janky(&deck, &["generate", "--yes"]);
    assert!(!missing.status.success());
    assert!(String::from_utf8_lossy(&missing.stderr).contains("the secret `api_key` of `voice`"));

    std::fs::write(project.join("key.txt"), "cli-key-0123456789\n").unwrap();
    let output = janky(&deck, &["generate", "--yes"]);
    server.join().unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{stderr}");
    assert!(
        stderr.contains("voice: 2 jobs\n  network: 127.0.0.1\n  secret `api_key`: file `key.txt`\n"),
        "{stderr}"
    );
    assert!(stderr.contains("the server refused the secret `api_key`"), "{stderr}");
    assert!(stderr.contains("HTTP 401 quota"), "{stderr}");
    assert!(stderr.contains("1 refused, 0 failed, 1 not started"), "{stderr}");
    assert!(!stderr.contains("cli-key-0123456789"));
}
