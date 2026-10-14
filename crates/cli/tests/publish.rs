mod common;

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::LazyLock;

use anki::collection::Collection;
use anki::sync::http_server::{SimpleServer, SyncServerConfig, default_ip_header};
use janky_anki::{Pulled, Session, open_copy};
use janky_cli::diagnostic::Diagnostics;
use janky_cli::{project, secrets};
use pbkdf2::password_hash::{PasswordHasher, SaltString};
use pbkdf2::{Params, Pbkdf2};

const DECK: &str = "[deck]\nname = \"Published ⚛️\"\n\n[publish]\nendpoint = \"https://anki.example.org/\"\nusername = \"example\"\npassword = { env = \"JANKY_TEST_SYNC_PASSWORD\" }\n";

#[test]
fn the_publish_password_comes_from_the_secret_sources() {
    let work = tempfile::tempdir().unwrap();
    std::fs::write(work.path().join("deck.toml"), DECK).unwrap();
    let mut diagnostics = Diagnostics::default();
    let project = project::load(&work.path().join("deck.toml"), &mut diagnostics).unwrap();
    assert!(secrets::publish_password(&project, &mut diagnostics).is_none());
    let text = diagnostics.to_json_lines();
    assert!(text.contains("secret::missing") && text.contains("the password of `[publish]`"), "{text}");

    std::fs::write(work.path().join(".env"), "JANKY_TEST_SYNC_PASSWORD=sync-password-0123\n").unwrap();
    let mut diagnostics = Diagnostics::default();
    assert_eq!(secrets::publish_password(&project, &mut diagnostics).as_deref(), Some("sync-password-0123"));
    assert!(diagnostics.is_empty());
    assert_eq!(janky_cli::diagnostic::redacted("x sync-password-0123 y"), "x <secret:password> y");
}

#[test]
fn resolving_plugin_secrets_also_hides_the_publish_password() {
    let work = tempfile::tempdir().unwrap();
    let deck = DECK.replace("JANKY_TEST_SYNC_PASSWORD", "JANKY_TEST_SYNC_PASSWORD_TWO");
    std::fs::write(work.path().join("deck.toml"), deck).unwrap();
    std::fs::write(
        work.path().join(".env"),
        "JANKY_TEST_SYNC_PASSWORD_TWO=plugin-cannot-see-9876
",
    )
    .unwrap();
    let mut diagnostics = Diagnostics::default();
    let checked =
        janky_cli::cli::check(&work.path().join("deck.toml"), Default::default(), &mut diagnostics, &mut Vec::new())
            .unwrap();
    assert_eq!(janky_cli::diagnostic::redacted("plugin-cannot-see-9876"), "plugin-cannot-see-9876");
    let mut diagnostics = Diagnostics::default();
    secrets::resolve(&checked.project, &checked.plugins, &mut diagnostics);
    assert!(diagnostics.is_empty(), "{}", diagnostics.to_json_lines());
    assert_eq!(janky_cli::diagnostic::redacted("plugin-cannot-see-9876"), "<secret:password>");
}

const USERS: usize = 13;

/// One in-process sync server for the test binary, as in janky-anki's sync tests. Each test uses
/// its own account. The passwords are PBKDF2 hashes with one round, so a debug build is fast.
static SERVER: LazyLock<String> = LazyLock::new(|| {
    let salt = SaltString::from_b64("tonuvYGpksNFQBlEmm3lxg").unwrap();
    let params = Params { rounds: 1, output_length: 32 };
    for user in 1..=USERS {
        let password = format!("password-{user}");
        let hash = Pbkdf2.hash_password_customized(password.as_bytes(), None, None, params, &salt).unwrap();
        unsafe { std::env::set_var(format!("SYNC_USER{user}"), format!("user{user}:{hash}")) };
    }
    unsafe { std::env::set_var("PASSWORDS_HASHED", "1") };
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        runtime.block_on(async {
            let base = tempfile::tempdir().unwrap();
            let (address, server) = SimpleServer::make_server(SyncServerConfig {
                host: "127.0.0.1".parse().unwrap(),
                port: 0,
                base_folder: base.path().into(),
                ip_header: default_ip_header(),
            })
            .await
            .unwrap();
            sender.send(format!("http://{address}/")).unwrap();
            server.await.unwrap();
        });
    });
    receiver.recv().unwrap()
});

/// A copy of the example project that publishes to the account of `user`.
fn project(user: usize) -> (tempfile::TempDir, PathBuf) {
    let endpoint = SERVER.clone();
    let (work, dir) = common::example();
    let deck = dir.join("deck.toml");
    let mut text = std::fs::read_to_string(&deck).unwrap();
    text.push_str(&format!(
        "\n[publish]\nendpoint = \"{endpoint}\"\nusername = \"user{user}\"\npassword = {{ env = \"JANKY_TEST_PUBLISH_PASSWORD\" }}\n"
    ));
    std::fs::write(&deck, text).unwrap();
    (work, deck)
}

fn publish(deck: &Path, user: usize, args: &[&str]) -> (Output, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_janky"))
        .arg("--deck")
        .arg(deck)
        .arg("publish")
        .args(args)
        .env("NO_COLOR", "1")
        .env("JANKY_TEST_PUBLISH_PASSWORD", format!("password-{user}"))
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    (output, stderr)
}

/// What another device gets from the server: a fresh collection after a pull.
fn phone(user: usize, dir: &Path) -> (Collection, Pulled) {
    let session = Session::login(&SERVER, &format!("user{user}"), &format!("password-{user}")).unwrap();
    session.pull(open_copy(&dir.join("phone.anki2")).unwrap()).unwrap()
}

fn notes(col: &Collection) -> i64 {
    col.storage.db().query_row("select count() from notes", [], |row| row.get(0)).unwrap()
}

#[test]
fn a_dry_run_changes_nothing_on_the_server() {
    let (work, deck) = project(1);
    let (output, stderr) = publish(&deck, 1, &["--dry-run"]);
    assert!(output.status.success(), "{stderr}");
    assert!(stderr.contains("Notes: 7 added, 0 changed, 0 kept, 0 removed, 0 unchanged"), "{stderr}");
    assert!(stderr.contains("Dry run: janky changed nothing on the server."), "{stderr}");
    let (phone, _) = phone(1, work.path());
    assert_eq!(notes(&phone), 0);
}

#[test]
fn publish_asks_and_needs_yes_without_a_terminal() {
    let (work, deck) = project(2);
    let (output, stderr) = publish(&deck, 2, &[]);
    assert!(!output.status.success());
    let plan = stderr.find("Notes: 7 added").expect(&stderr);
    let question = stderr.find("publish::confirm").expect(&stderr);
    assert!(plan < question, "the plan must come before the question: {stderr}");
    let (phone, _) = phone(2, work.path());
    assert_eq!(notes(&phone), 0);
}

#[test]
fn publish_reaches_other_devices_and_a_second_run_changes_nothing() {
    let (work, deck) = project(3);
    let (output, stderr) = publish(&deck, 3, &["--yes"]);
    assert!(output.status.success(), "{stderr}");
    assert!(stderr.contains("Published: 7 notes added"), "{stderr}");
    assert!(stderr.contains("Download replaces the collection on that device"), "{stderr}");
    assert!(!stderr.contains("choose to download"), "{stderr}");
    let root = deck.parent().unwrap();
    assert!(root.join(".janky/publish/state.json").is_file());
    assert!(!root.join(".janky/publish/applying").exists());

    let (phone, pulled) = phone(3, work.path());
    assert_eq!((notes(&phone), pulled), (7, Pulled::FullDownload));
    let session = Session::login(&SERVER, "user3", "password-3").unwrap();
    session.sync_media(&phone).unwrap();
    assert!(work.path().join("phone.media/diamond.png").is_file());

    let (output, stderr) = publish(&deck, 3, &["--yes"]);
    assert!(output.status.success(), "{stderr}");
    assert!(stderr.contains("No changes."), "{stderr}");
}

#[test]
fn a_schema_change_needs_allow_full_sync() {
    let (work, deck) = project(4);
    let (output, stderr) = publish(&deck, 4, &["--yes"]);
    assert!(output.status.success(), "{stderr}");
    let notetype = deck.parent().unwrap().join("notetypes/element/notetype.toml");
    let mut text = std::fs::read_to_string(&notetype).unwrap();
    text.push_str("\n[fields.extra]\nname = \"Extra\"\n");
    std::fs::write(&notetype, text).unwrap();

    let (output, stderr) = publish(&deck, 4, &["--yes"]);
    assert!(!output.status.success());
    assert!(stderr.contains("publish::full-sync") && stderr.contains("Needs a full sync"), "{stderr}");
    let (output, stderr) = publish(&deck, 4, &["--yes", "--allow-full-sync"]);
    assert!(output.status.success(), "{stderr}");
    assert!(stderr.contains("choose to download"), "{stderr}");
    let (phone, pulled) = phone(4, work.path());
    assert_eq!((notes(&phone), pulled), (7, Pulled::FullDownload));
}

#[test]
fn a_removed_entry_stays_unless_delete_removed() {
    let (work, deck) = project(5);
    let (output, stderr) = publish(&deck, 5, &["--yes"]);
    assert!(output.status.success(), "{stderr}");
    let early = deck.parent().unwrap().join("content/periods/early.yaml");
    let text = std::fs::read_to_string(&early).unwrap();
    let cut = text.find("  - id: 'c'").unwrap();
    std::fs::write(&early, &text[..cut]).unwrap();

    let (output, stderr) = publish(&deck, 5, &["--yes"]);
    assert!(output.status.success(), "{stderr}");
    assert!(stderr.contains("- note c: stays in Anki. `--delete-removed` deletes it"), "{stderr}");
    assert_eq!(notes(&phone(5, work.path()).0), 7);
    let (output, stderr) = publish(&deck, 5, &["--yes", "--delete-removed"]);
    assert!(output.status.success(), "{stderr}");
    assert!(stderr.contains("1 deleted"), "{stderr}");
    std::fs::remove_file(work.path().join("phone.anki2")).unwrap();
    assert_eq!(notes(&phone(5, work.path()).0), 6);
}

#[test]
fn a_wrong_password_stops_at_the_login() {
    let (_work, deck) = project(6);
    let output = Command::new(env!("CARGO_BIN_EXE_janky"))
        .arg("--deck")
        .arg(&deck)
        .args(["publish", "--yes"])
        .env("NO_COLOR", "1")
        .env("JANKY_TEST_PUBLISH_PASSWORD", "not-the-password")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(
        stderr.contains("publish::login") && stderr.contains("refused the user name or the password"),
        "{stderr}"
    );
    assert!(!stderr.contains("not-the-password"), "{stderr}");
}

fn deck_cards(col: &mut Collection, name: &str) -> i64 {
    let Some(id) = col.get_deck_id(name).unwrap() else { return -1 };
    col.storage.db().query_row("select count() from cards where did = ?", [id.0], |row| row.get(0)).unwrap()
}

#[test]
fn a_renamed_deck_keeps_its_cards_on_other_devices() {
    let (work, deck) = project(7);
    let (output, stderr) = publish(&deck, 7, &["--yes"]);
    assert!(output.status.success(), "{stderr}");
    let text = std::fs::read_to_string(&deck).unwrap().replace("name = \"0 Elements\"", "name = \"0 Elements 🧪\"");
    std::fs::write(&deck, text).unwrap();
    let (output, stderr) = publish(&deck, 7, &["--yes"]);
    assert!(output.status.success(), "{stderr}");
    assert!(
        stderr.contains("~ deck Periodic Table::0 Elements is renamed to Periodic Table::0 Elements 🧪"),
        "{stderr}"
    );
    let (mut phone, _) = phone(7, work.path());
    assert_eq!(phone.get_deck_id("Periodic Table::0 Elements").unwrap(), None);
    assert!(deck_cards(&mut phone, "Periodic Table::0 Elements 🧪") > 0);
}

#[test]
fn entries_that_move_to_another_deck_move_their_cards() {
    let (work, deck) = project(8);
    let (output, stderr) = publish(&deck, 8, &["--yes"]);
    assert!(output.status.success(), "{stderr}");
    let root = deck.parent().unwrap();
    let text = std::fs::read_to_string(&deck).unwrap().replace(
        "content = [\"content/periods/*.yaml\", \"content/periods/*.json\"]",
        "content = [\"content/periods/*.yaml\"]",
    );
    let text = text.replace(
        "[deck.children.groups]",
        "[deck.children.extra]\nname = \"2 Extra\"\ncontent = [\"content/extra/*.json\"]\n[deck.children.groups]",
    );
    std::fs::write(&deck, text).unwrap();
    std::fs::create_dir_all(root.join("content/extra")).unwrap();
    std::fs::rename(root.join("content/periods/late.json"), root.join("content/extra/late.json")).unwrap();

    let (output, stderr) = publish(&deck, 8, &["--yes"]);
    assert!(output.status.success(), "{stderr}");
    assert!(stderr.contains("+ deck Periodic Table::2 Extra") && stderr.contains("cards move"), "{stderr}");
    let (mut phone, _) = phone(8, work.path());
    assert!(deck_cards(&mut phone, "Periodic Table::2 Extra") > 0);
}

#[test]
fn a_run_that_stopped_early_starts_from_a_fresh_copy() {
    let (work, deck) = project(9);
    let (output, stderr) = publish(&deck, 9, &["--yes"]);
    assert!(output.status.success(), "{stderr}");
    std::fs::write(deck.parent().unwrap().join(".janky/publish/applying"), b"").unwrap();
    let (output, stderr) = publish(&deck, 9, &["--yes"]);
    assert!(output.status.success(), "{stderr}");
    assert!(stderr.contains("The last publish stopped before it ended"), "{stderr}");
    assert!(stderr.contains("No changes."), "{stderr}");
    assert_eq!(notes(&phone(9, work.path()).0), 7);
}

#[test]
fn another_account_starts_from_a_fresh_copy() {
    let (work, deck) = project(10);
    let (output, stderr) = publish(&deck, 10, &["--yes"]);
    assert!(output.status.success(), "{stderr}");
    let text = std::fs::read_to_string(&deck).unwrap().replace("username = \"user10\"", "username = \"user11\"");
    std::fs::write(&deck, text).unwrap();
    let (output, stderr) = publish(&deck, 11, &["--yes"]);
    assert!(output.status.success(), "{stderr}");
    assert!(stderr.contains("The sync server or the account changed"), "{stderr}");
    assert!(stderr.contains("Published: 7 notes added"), "{stderr}");
    assert_eq!(notes(&phone(11, work.path()).0), 7);
}

#[test]
fn a_link_in_the_copy_directory_is_refused() {
    let (work, deck) = project(1);
    let outside = work.path().join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    let dir = deck.parent().unwrap().join(".janky/publish");
    std::fs::create_dir_all(&dir).unwrap();
    std::os::unix::fs::symlink(&outside, dir.join("collection.media")).unwrap();
    let (output, stderr) = publish(&deck, 1, &["--yes"]);
    assert!(!output.status.success());
    assert!(stderr.contains("publish::write") && stderr.contains("is a symbolic link"), "{stderr}");
    assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 0);
}

#[test]
fn media_pull_gets_the_generated_media_of_a_fresh_clone() {
    let (work, deck) = project(12);
    let (output, stderr) = publish(&deck, 12, &["--yes"]);
    assert!(output.status.success(), "{stderr}");

    let clone = work.path().join("clone");
    common::copy_example(&clone);
    std::fs::copy(&deck, clone.join("deck.toml")).unwrap();
    let pull = || {
        let output = Command::new(env!("CARGO_BIN_EXE_janky"))
            .arg("--deck")
            .arg(clone.join("deck.toml"))
            .arg("pull")
            .env("NO_COLOR", "1")
            .env("JANKY_TEST_PUBLISH_PASSWORD", "password-12")
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        (output, stderr)
    };
    let (output, stderr) = pull();
    assert!(output.status.success(), "{stderr}");
    assert!(stderr.contains("data results are missing. They are not on the server"), "{stderr}");

    let cache = |root: &Path| {
        let mut files: Vec<(String, Vec<u8>)> = std::fs::read_dir(root.join(".janky/cache/media"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|ext| ext != "json"))
            .map(|path| (path.file_name().unwrap().to_string_lossy().to_string(), std::fs::read(&path).unwrap()))
            .collect();
        files.sort();
        files
    };
    let original = cache(deck.parent().unwrap());
    assert!(!original.is_empty());
    assert!(stderr.contains(&format!("Pulled {} media files from the server.", original.len())), "{stderr}");
    assert_eq!(cache(&clone), original);

    let (kept, _) = &original[0];
    let leftover = work.path().join("leftover");
    common::copy_example(&leftover);
    std::fs::copy(&deck, leftover.join("deck.toml")).unwrap();
    std::fs::create_dir_all(leftover.join(".janky/cache/media")).unwrap();
    std::fs::write(leftover.join(".janky/cache/media").join(kept), b"paid, without metadata").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_janky"))
        .arg("--deck")
        .arg(leftover.join("deck.toml"))
        .arg("pull")
        .env("NO_COLOR", "1")
        .env("JANKY_TEST_PUBLISH_PASSWORD", "password-12")
        .output()
        .unwrap();
    let leftover_stderr = String::from_utf8_lossy(&output.stderr);
    assert!(leftover_stderr.contains("already in the cache without its metadata"), "{leftover_stderr}");
    assert_eq!(std::fs::read(leftover.join(".janky/cache/media").join(kept)).unwrap(), b"paid, without metadata");

    let (output, stderr) = pull();
    assert!(output.status.success(), "{stderr}");
    assert!(stderr.contains("No generated media files are missing."), "{stderr}");
}

#[test]
fn media_pull_refuses_a_file_that_does_not_match_deck_lock() {
    let (work, deck) = project(13);
    let (output, stderr) = publish(&deck, 13, &["--yes"]);
    assert!(output.status.success(), "{stderr}");

    let clone = work.path().join("clone");
    common::copy_example(&clone);
    std::fs::copy(&deck, clone.join("deck.toml")).unwrap();
    let lock = std::fs::read_to_string(clone.join("deck.lock")).unwrap();
    let line = lock.lines().find(|line| line.contains(".ogg\" = { hash = ")).unwrap().to_string();
    let name = line.split('"').nth(1).unwrap().to_string();
    let tampered = format!("\"{name}\" = {{ hash = \"0000000000000000\" }}");
    std::fs::write(clone.join("deck.lock"), lock.replace(&line, &tampered)).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_janky"))
        .arg("--deck")
        .arg(clone.join("deck.toml"))
        .arg("pull")
        .env("NO_COLOR", "1")
        .env("JANKY_TEST_PUBLISH_PASSWORD", "password-13")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(stderr.contains(&format!("`{name}` from the server does not match deck.lock")), "{stderr}");
    assert!(!clone.join(".janky/cache/media").join(&name).exists());
}
