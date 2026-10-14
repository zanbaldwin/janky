use std::path::Path;
use std::sync::LazyLock;

use anki::collection::Collection;
use anki::decks::DeckId;
use anki::notes::NoteId;
use anki::notetype::NoteField;
use anki::sync::http_server::{SimpleServer, SyncServerConfig, default_ip_header};
use janky_anki::{
    Apply, Card, Deck, Field, Note, Notetype, Package, Preset, PresetOptions, Pulled, Pushed, Session, apply, open_copy,
};
use pbkdf2::password_hash::{PasswordHasher, SaltString};
use pbkdf2::{Params, Pbkdf2};

const USERS: usize = 6;

/// One in-process sync server for the whole test binary. Each test uses its own account.
///
/// The server reads its accounts from `SYNC_USER<n>`. Every test calls this first, so the
/// variables are set before any test thread reads the environment. The passwords are stored as
/// PBKDF2 hashes with one round, because the server's default hash takes seconds in a debug
/// build.
static SERVER: LazyLock<String> = LazyLock::new(|| {
    let salt = SaltString::from_b64("tonuvYGpksNFQBlEmm3lxg").unwrap();
    let params = Params { rounds: 1, output_length: 32 };
    for user in 1..=USERS {
        let password = format!("password-{user}");
        let hash = Pbkdf2.hash_password_customized(password.as_bytes(), None, None, params, &salt).unwrap();
        unsafe { std::env::set_var(format!("SYNC_USER{user}"), format!("user{user}:{hash}")) };
    }
    unsafe { std::env::set_var("PASSWORDS_HASHED", "1") };
    // A proxy that nothing listens on: a client that uses the proxy variables fails every test.
    for variable in ["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "http_proxy", "all_proxy"] {
        unsafe { std::env::set_var(variable, "http://127.0.0.1:9") };
    }
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

fn login(user: usize) -> Session {
    Session::login(&SERVER, &format!("user{user}"), &format!("password-{user}")).unwrap()
}

fn notes(col: &Collection) -> i64 {
    col.storage.db().query_row("select count() from notes", [], |row| row.get(0)).unwrap()
}

fn first_field(col: &Collection) -> String {
    let fields: String = col.storage.db().query_row("select flds from notes", [], |row| row.get(0)).unwrap();
    fields.split('\u{1f}').next().unwrap().to_string()
}

fn add_basic(col: &mut Collection, text: &str) {
    let basic = col.get_notetype_by_name("Basic").unwrap().unwrap();
    let mut note = basic.new_note();
    note.set_field(0, text).unwrap();
    col.add_note(&mut note, DeckId(1)).unwrap();
}

/// A "phone" that already has one note and one image on the server.
fn phone(session: &Session, dir: &Path) -> Collection {
    let mut phone = open_copy(&dir.join("phone.anki2")).unwrap();
    add_basic(&mut phone, "from the phone <img src=\"phone.png\">");
    std::fs::write(dir.join("phone.media/phone.png"), b"png").unwrap();
    assert_eq!(session.push(&mut phone).unwrap(), Pushed::FullSyncRequired { server_empty: true });
    let phone = session.full_upload(phone).unwrap();
    session.sync_media(&phone).unwrap();
    phone
}

#[test]
fn a_wrong_password_is_refused_with_a_plain_message() {
    let endpoint = SERVER.clone();
    let error = Session::login(&endpoint, "user1", "not-the-password").err().unwrap();
    assert_eq!(error.0, "the sync server refused the user name or the password");
}

#[test]
fn the_first_pull_of_an_empty_copy_is_a_full_download() {
    let session = login(2);
    let work = tempfile::tempdir().unwrap();
    phone(&session, work.path());

    let (copy, pulled) = session.pull(open_copy(&work.path().join("copy.anki2")).unwrap()).unwrap();
    assert_eq!(pulled, Pulled::FullDownload);
    assert_eq!(notes(&copy), 1);
    session.sync_media(&copy).unwrap();
    assert_eq!(std::fs::read(work.path().join("copy.media/phone.png")).unwrap(), b"png");
}

#[test]
fn a_pull_gets_an_edit_from_another_client() {
    let session = login(3);
    let work = tempfile::tempdir().unwrap();
    let mut phone = phone(&session, work.path());
    let (copy, _) = session.pull(open_copy(&work.path().join("copy.anki2")).unwrap()).unwrap();

    let id: i64 = phone.storage.db().query_row("select id from notes", [], |row| row.get(0)).unwrap();
    let mut note = phone.storage.get_note(NoteId(id)).unwrap().unwrap();
    note.set_field(0, "edited on the phone").unwrap();
    phone.update_note(&mut note).unwrap();
    assert_eq!(session.push(&mut phone).unwrap(), Pushed::Done);

    let (copy, pulled) = session.pull(copy).unwrap();
    assert_eq!(pulled, Pulled::Normal);
    assert_eq!(first_field(&copy), "edited on the phone");
}

#[test]
fn a_push_reaches_the_other_client() {
    let session = login(4);
    let work = tempfile::tempdir().unwrap();
    let phone = phone(&session, work.path());
    let (mut copy, _) = session.pull(open_copy(&work.path().join("copy.anki2")).unwrap()).unwrap();

    add_basic(&mut copy, "from janky [sound:janky.mp3]");
    std::fs::write(work.path().join("copy.media/janky.mp3"), b"mp3").unwrap();
    assert_eq!(session.push(&mut copy).unwrap(), Pushed::Done);
    session.sync_media(&copy).unwrap();

    let (phone, pulled) = session.pull(phone).unwrap();
    assert_eq!(pulled, Pulled::Normal);
    assert_eq!(notes(&phone), 2);
    session.sync_media(&phone).unwrap();
    assert_eq!(std::fs::read(work.path().join("phone.media/janky.mp3")).unwrap(), b"mp3");
}

#[test]
fn a_schema_change_needs_a_full_sync_and_sends_nothing() {
    let session = login(5);
    let work = tempfile::tempdir().unwrap();
    let phone = phone(&session, work.path());
    let (mut copy, _) = session.pull(open_copy(&work.path().join("copy.anki2")).unwrap()).unwrap();

    let mut basic = (*copy.get_notetype_by_name("Basic").unwrap().unwrap()).clone();
    basic.fields.push(NoteField::new("Extra"));
    copy.update_notetype(&mut basic, false).unwrap();
    add_basic(&mut copy, "not sent");
    assert_eq!(session.push(&mut copy).unwrap(), Pushed::FullSyncRequired { server_empty: false });

    let (phone, pulled) = session.pull(phone).unwrap();
    assert_eq!(pulled, Pulled::Normal);
    assert_eq!(notes(&phone), 1);

    // The server compares modification times in milliseconds first. If the upload lands in the
    // same millisecond as the phone's last sync, the phone sees no change at all.
    std::thread::sleep(std::time::Duration::from_millis(5));
    let copy = session.full_upload(copy).unwrap();
    assert_eq!(notes(&copy), 2);
    let (phone, pulled) = session.pull(phone).unwrap();
    assert_eq!(pulled, Pulled::FullDownload);
    assert_eq!(notes(&phone), 2);
}

#[test]
fn a_redirect_does_not_carry_the_login_to_another_host() {
    let _ = &*SERVER;
    let target = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    target.set_nonblocking(true).unwrap();
    let location = format!("http://{}/stolen", target.local_addr().unwrap());
    let redirector = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}/", redirector.local_addr().unwrap());
    std::thread::spawn(move || {
        for stream in redirector.incoming() {
            let mut stream = stream.unwrap();
            let mut buffer = [0u8; 4096];
            let _ = std::io::Read::read(&mut stream, &mut buffer);
            let reply = format!("HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\n\r\n");
            let _ = std::io::Write::write_all(&mut stream, reply.as_bytes());
        }
    });

    assert!(Session::login(&endpoint, "user1", "password-1").is_err());
    std::thread::sleep(std::time::Duration::from_millis(100));
    assert!(target.accept().is_err(), "the redirect target got a connection");
}

fn package(deck_name: &str, new_per_day: u32) -> Package {
    Package {
        notetypes: vec![Notetype {
            id: 1001,
            name: "Type".into(),
            css: String::new(),
            sort_field: 0,
            modified: 1000,
            fields: vec![Field {
                id: 11,
                name: "Front".into(),
                description: String::new(),
                collapsed: false,
            }],
            cards: vec![Card {
                id: 21,
                name: "Card 1".into(),
                front: "{{Front}}".into(),
                back: "x".into(),
            }],
        }],
        presets: vec![Preset {
            id: 200,
            name: "Lessons".into(),
            options: PresetOptions {
                new_per_day: Some(new_per_day),
                ..Default::default()
            },
        }],
        decks: vec![Deck {
            id: 101,
            name: deck_name.into(),
            preset: Some(200),
        }],
        notes: vec![Note {
            guid: "n1".into(),
            notetype: 1001,
            deck: 101,
            fields: vec!["one".into()],
            tags: vec![],
            modified: 1000,
        }],
        media: vec![],
    }
}

#[test]
fn applied_changes_reach_another_client() {
    let session = login(6);
    let work = tempfile::tempdir().unwrap();
    let phone = phone(&session, work.path());
    let (mut copy, _) = session.pull(open_copy(&work.path().join("copy.anki2")).unwrap()).unwrap();

    let applied = apply(&mut copy, &package("Elements", 15), &Apply::default()).unwrap();
    assert_eq!(session.push(&mut copy).unwrap(), Pushed::Done);
    let (mut copy, _) = session.pull(copy).unwrap();
    let request = Apply {
        decks: applied.decks.clone(),
        ..Default::default()
    };
    apply(&mut copy, &package("Periodic Table", 30), &request).unwrap();
    assert_eq!(session.push(&mut copy).unwrap(), Pushed::Done);

    let (phone, _) = session.pull(phone).unwrap();
    assert_eq!(notes(&phone), 2);
    assert_eq!(phone.get_deck_id("Elements").unwrap(), None);
    let deck = phone.get_deck_id("Periodic Table").unwrap().unwrap();
    assert_eq!(deck.0, applied.decks[&101]);
    let preset = Collection::get_deck_config(&phone, anki::deckconfig::DeckConfigId(200), false).unwrap().unwrap();
    assert_eq!(preset.inner.new_per_day, 30);
}
