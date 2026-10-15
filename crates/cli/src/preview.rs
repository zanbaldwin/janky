//! `janky preview`: a local web server that shows each card of the project in the browser.
//!
//! The server listens on `127.0.0.1` only. It refuses a request whose `Host` header is not that address and port, so
//! that a web page cannot reach the server through DNS rebinding. All routes are `GET`.
//!
//! Each `GET /api/preview` runs the pipeline again on the current files: [`check`], `deck.lock`,
//! [`package::package_with`] on the lock file in memory, and [`janky_anki::render`]. It does not read the media
//! files, so a large media directory does not slow down a page load. The preview writes no files, and generators
//! never run. Each note type that no entry uses gets one sample note, which rslib fills with
//! `(Field name)`.
//!
//! Each run gets a new generation: 16 random bytes in hex. The server keeps the rows, the rendered cards and the
//! media files of the last run that made a package. If the project, the package or the render fails, the response
//! has the new diagnostics, the old rows, and `"stale": true`, so that the author still sees the last good cards.
//!
//! `/card/{generation}/{row}/{side}` is the document of one card side for the iframe of the page. Like Anki's
//! reviewer, the `card` entry of the UI build puts the card HTML into `#qa`, runs its scripts, and runs the
//! `onUpdateHook` and `onShownHook` callbacks. `/media/{generation}/{name}` serves a media file of the snapshot by its
//! Anki name. A URL never becomes a file path. A request for an older generation gets `410 Gone`.
//!
//! The iframe has an opaque origin, so its module scripts and fonts need `Access-Control-Allow-Origin`. Another web
//! page can send requests to `127.0.0.1` too, but it cannot read `/api/preview`, so it cannot know the random
//! generation in the media URLs. The card document and the media files also get a `sandbox` policy, so that they
//! never run in the origin of the page, also when the user opens them directly.
//!
//! The pipeline runs on one blocking thread at a time, behind a mutex, because each run loads the plugins into a new
//! Lua sandbox. The Lua watchdog stops the whole process if a plugin passes the time limit.
//!
//! The UI comes from `ui/dist/`, which `build.rs` embeds. A debug build without the UI shows a message at `/`.

use std::collections::{HashMap, HashSet};
use std::net::{Ipv4Addr, SocketAddr, TcpListener};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::Router;
use axum::extract::{self, Request, State};
use axum::http::{StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use janky_anki::{Package, RenderedCard, Sound};
use serde::Deserialize;
use serde_json::{Value, json};
use vite_chunks::Manifest;

use crate::cli::{Checked, check, missing_media};
use crate::diagnostic::{Diagnostic, Diagnostics, json_text, visible};
use crate::lock::{self, LockFile};
use crate::{package, project};

/// The files of the UI build, by their URL path without the first `/`.
static ASSETS: &[(&str, &[u8])] = include!(concat!(env!("OUT_DIR"), "/assets.rs"));

/// The Vite manifest of the UI build. It is empty if the UI is not built.
static MANIFEST: LazyLock<Manifest> = LazyLock::new(|| {
    include_str!(concat!(env!("OUT_DIR"), "/vite-manifest.json")).parse().expect("the Vite manifest to be valid")
});

const NOT_BUILT: &str = "<!doctype html><meta charset=\"utf-8\"><title>janky preview</title>\
<p>The preview UI is not built. Run <code>just assets</code>, then build janky again.</p>";

/// The policy of the page and the API. The card documents of the iframe get none, because card JS must work as in
/// Anki.
const PAGE_POLICY: &str = "default-src 'self'; object-src 'none'; base-uri 'none'; frame-ancestors 'none'";
const API_POLICY: &str = "default-src 'none'; frame-ancestors 'none'";
const CARD_POLICY: &str = "sandbox allow-scripts";
const MEDIA_POLICY: &str = "default-src 'none'; style-src 'unsafe-inline'; sandbox";

/// A bound preview server that does not serve yet.
pub struct Server {
    listener: TcpListener,
    deck: PathBuf,
    options: project::Options,
}

struct Shared {
    deck: PathBuf,
    options: project::Options,
    port: u16,
    /// Held while the pipeline runs, so that only one run happens at a time.
    pipeline: Mutex<()>,
    snapshot: Mutex<Snapshot>,
}

#[derive(Default)]
struct Snapshot {
    /// Empty before the first run.
    generation: String,
    rows: Vec<Value>,
    /// One for each row.
    cards: Vec<Card>,
    /// The media files by Anki name.
    media: HashMap<String, PathBuf>,
}

/// What a card document needs from one row.
struct Card {
    css: String,
    ord: usize,
    /// `None` if the card did not render. The row has the error.
    rendered: Option<RenderedCard>,
}

/// The project as the preview loads it: the result of [`check`], the lock file in memory, and the package.
pub(crate) struct Loaded {
    pub checked: Checked,
    pub lock: LockFile,
    pub package: Package,
}

/// The cards that [`render`] renders.
pub(crate) enum Selection<'a> {
    /// Every note, then one sample note for each note type that no entry uses.
    All,
    /// The note of the entry with this id.
    Entry(&'a str),
    /// One sample note of the note type with this key.
    Sample(&'a str),
}

/// One rendered card, with what the table, the documents and `janky show` need.
pub(crate) struct Row {
    pub notetype: String,
    pub notetype_name: String,
    /// `None` for a sample note.
    pub entry: Option<RowEntry>,
    /// The deck key and the full deck name. `None` for a sample note.
    pub deck: Option<(String, String)>,
    pub card_key: String,
    pub card_name: String,
    pub ord: usize,
    pub css: String,
    /// The error message if the card did not render.
    pub rendered: std::result::Result<RenderedCard, String>,
}

/// The entry of a [`Row`].
pub(crate) struct RowEntry {
    pub id: String,
    pub file: String,
    /// The position in the `entries` of the file, from 0.
    pub index: Option<usize>,
}

#[derive(Deserialize)]
struct CardQuery {
    night: Option<String>,
    platform: Option<String>,
    /// The typed answer, for the back of a card with `{{type:Field}}`.
    typed: Option<String>,
}

impl Server {
    /// Listens on `127.0.0.1:port` for the project of `deck` (the path of `deck.toml`). Port `0` lets the operating
    /// system choose a port.
    pub fn bind(deck: &Path, options: project::Options, port: u16) -> Result<Server, String> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port))
            .and_then(|listener| listener.set_nonblocking(true).map(|()| listener))
            .map_err(|error| format!("cannot listen on `127.0.0.1:{port}`: {error}"))?;
        Ok(Server {
            listener,
            deck: deck.to_path_buf(),
            options,
        })
    }

    /// The address that the server listens on.
    pub fn address(&self) -> SocketAddr {
        self.listener.local_addr().expect("a bound listener has an address")
    }

    /// Serves requests until the process stops. It returns only if the server fails.
    pub fn run(self) -> Result<(), String> {
        let port = self.address().port();
        let shared = Arc::new(Shared {
            deck: self.deck,
            options: self.options,
            port,
            pipeline: Mutex::default(),
            snapshot: Mutex::default(),
        });
        let app = Router::new()
            .route("/", get(index))
            .route("/assets/{*path}", get(asset))
            .route("/api/preview", get(api))
            .route("/card/{generation}/{row}/{side}", get(card))
            .route("/media/{generation}/{name}", get(media))
            .layer(middleware::from_fn_with_state(shared.clone(), host))
            .with_state(shared);
        let runtime =
            tokio::runtime::Builder::new_current_thread().enable_io().build().map_err(|error| error.to_string())?;
        runtime.block_on(async {
            let listener = tokio::net::TcpListener::from_std(self.listener).map_err(|error| error.to_string())?;
            axum::serve(listener, app).await.map_err(|error| error.to_string())
        })
    }
}

/// Runs `janky preview` until the process stops. A failure to start or to serve goes into `diagnostics`.
pub fn serve(deck: &Path, options: project::Options, port: u16, diagnostics: &mut Diagnostics) {
    let server = match Server::bind(deck, options, port) {
        Ok(server) => server,
        Err(message) => {
            diagnostics.push(Diagnostic::error("preview::bind", message).with_help("use another port with `--port`"));
            return;
        },
    };
    eprintln!(
        "{}",
        visible(&format!(
            "Preview at http://{}/ \u{2014} reload the page to read the files again, press Ctrl+C to stop",
            server.address()
        ))
    );
    if let Err(message) = server.run() {
        diagnostics.push(Diagnostic::error("preview::serve", message));
    }
}

async fn host(State(shared): State<Arc<Shared>>, request: Request, next: Next) -> Response {
    let port = shared.port;
    let allowed = request
        .headers()
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|host| host == format!("127.0.0.1:{port}") || host == format!("localhost:{port}"));
    if !allowed {
        return (StatusCode::MISDIRECTED_REQUEST, "the Host header is not the preview server").into_response();
    }
    next.run(request).await
}

async fn index() -> Response {
    match ASSETS.iter().find(|(path, _)| *path == "index.html") {
        Some((path, bytes)) => file(path, bytes),
        None => file("index.html", NOT_BUILT.as_bytes()),
    }
}

async fn asset(extract::Path(path): extract::Path<String>) -> Response {
    let path = format!("assets/{path}");
    match ASSETS.iter().find(|(asset, _)| *asset == path) {
        Some((path, bytes)) => file(path, bytes),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn api(State(shared): State<Arc<Shared>>) -> Response {
    match tokio::task::spawn_blocking(move || shared.refresh()).await {
        Ok(body) => (
            [
                (header::CONTENT_TYPE, "application/json"),
                (header::CACHE_CONTROL, "no-store"),
                (header::CONTENT_SECURITY_POLICY, API_POLICY),
                (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
            ],
            body,
        )
            .into_response(),
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "the preview pipeline failed").into_response(),
    }
}

async fn card(
    State(shared): State<Arc<Shared>>,
    extract::Path((generation, row, side)): extract::Path<(String, usize, String)>,
    extract::Query(query): extract::Query<CardQuery>,
) -> Response {
    let question = match side.as_str() {
        "front" => true,
        "back" => false,
        _ => return (StatusCode::NOT_FOUND, "the side must be `front` or `back`").into_response(),
    };
    let night = match query.night.as_deref() {
        None | Some("0") => false,
        Some("1") => true,
        Some(_) => return (StatusCode::BAD_REQUEST, "`night` must be `0` or `1`").into_response(),
    };
    let platform = match query.platform.as_deref() {
        None | Some("desktop") => Platform::Desktop,
        Some("android") => Platform::Android,
        Some("iphone") => Platform::Iphone,
        Some(_) => {
            return (StatusCode::BAD_REQUEST, "`platform` must be `desktop`, `android` or `iphone`").into_response();
        },
    };
    let body = {
        let snapshot = shared.snapshot.lock().unwrap_or_else(PoisonError::into_inner);
        if snapshot.generation.is_empty() || snapshot.generation != generation {
            return (StatusCode::GONE, "this card is from an earlier page load: reload the page").into_response();
        }
        let Some(card) = snapshot.cards.get(row) else {
            return (StatusCode::NOT_FOUND, "no such row").into_response();
        };
        let Some(rendered) = &card.rendered else {
            return (StatusCode::NOT_FOUND, "the card did not render").into_response();
        };
        let body_class = body_class(card.ord, night, platform);
        if question {
            document(&generation, &card.css, &rendered.question.html, &rendered.question.sounds, &body_class, &side)
        } else {
            let typed = query.typed.as_deref().unwrap_or_default();
            let html = match janky_anki::compare_typed(&rendered.answer.html, rendered.type_answer.as_ref(), typed) {
                Ok(html) => html,
                Err(error) => return (StatusCode::INTERNAL_SERVER_ERROR, error.0).into_response(),
            };
            document(&generation, &card.css, &html, &rendered.answer.sounds, &body_class, &side)
        }
    };
    (
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (header::CACHE_CONTROL, "no-store"),
            (header::CONTENT_SECURITY_POLICY, CARD_POLICY),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        ],
        body,
    )
        .into_response()
}

/// The card document. The card data is JSON in a `<script>` element, with each `<` escaped, so that card HTML cannot
/// end that element. `body_class` and `side` are fixed strings, and `generation` is hex.
fn document(generation: &str, css: &str, html: &str, sounds: &[Sound], body_class: &str, side: &str) -> String {
    let data = card_data(&card_json(css, html, sounds, body_class, side));
    let tags = MANIFEST.entry("card").map(|entry| entry.to_html()).unwrap_or_default();
    format!(
        "<!doctype html>\n<html>\n<head>\n<meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
         <base href=\"/media/{generation}/\">\n{tags}</head>\n<body class=\"{body_class}\">\n<div id=\"qa\"></div>\n\
         <script type=\"application/json\" id=\"janky-card\">{data}</script>\n</body>\n</html>\n"
    )
}

/// The card data of the shim, before [`card_data`].
pub(crate) fn card_json(css: &str, html: &str, sounds: &[Sound], body_class: &str, side: &str) -> Value {
    let html = format!("<style>{css}</style>{html}");
    json!({ "html": html, "bodyclass": body_class, "side": side, "sounds": sounds_json(sounds) })
}

/// The card data as the text of a `<script type="application/json">` element. Each `<` is escaped, so that card
/// HTML cannot end the element.
fn card_data(data: &Value) -> String {
    json_text(data).replace('<', "\\u003c")
}

/// The platform that the classes of the card body copy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub enum Platform {
    #[default]
    Desktop,
    Android,
    Iphone,
}

/// The classes of the card body, as Anki's reviewer sets them.
pub(crate) fn body_class(ord: usize, night: bool, platform: Platform) -> String {
    let platform = match platform {
        Platform::Desktop => "isLin",
        Platform::Android => "mobile android",
        Platform::Iphone => "mobile iphone",
    };
    let night = if night { " nightMode night_mode" } else { "" };
    format!("card card{} {platform}{night}", ord + 1)
}

/// Whether this janky has the UI build, which the card documents need.
pub fn ui_built() -> bool {
    MANIFEST.entry("card").is_some()
}

/// A card document that needs no server, for `janky show --format html`. The CSS and the JS of the `card` entry are
/// in the document, and `media` maps each media name to a `file://` URL. It returns `None` without the UI build.
///
/// `build.rs` makes sure that the `card` entry is one JS file, and that its JS and CSS can be inline.
pub(crate) fn file_document(data: &Value, body_class: &str) -> Option<String> {
    let entry = MANIFEST.entry("card")?;
    let bytes = |url: String| {
        let path = url.trim_start_matches('/').to_string();
        ASSETS.iter().find(|(asset, _)| *asset == path).map(|(_, bytes)| String::from_utf8_lossy(bytes).into_owned())
    };
    let script = bytes(entry.script?.to_string())?;
    let styles: Vec<String> = entry.stylesheets.iter().filter_map(|url| bytes(url.to_string())).collect();
    let styles = styles.join("\n");
    let data = card_data(data);
    Some(format!(
        "<!doctype html>\n<html>\n<head>\n<meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<style>\n{styles}\n</style>\n</head>\n\
         <body class=\"{body_class}\">\n<div id=\"qa\"></div>\n\
         <script type=\"application/json\" id=\"janky-card\">{data}</script>\n<script type=\"module\">\n{script}\n</script>\n\
         </body>\n</html>\n"
    ))
}

async fn media(
    State(shared): State<Arc<Shared>>,
    extract::Path((generation, name)): extract::Path<(String, String)>,
) -> Response {
    let path = {
        let snapshot = shared.snapshot.lock().unwrap_or_else(PoisonError::into_inner);
        if snapshot.generation.is_empty() || snapshot.generation != generation {
            return StatusCode::GONE.into_response();
        }
        snapshot.media.get(&name).cloned()
    };
    let Some(path) = path else {
        return StatusCode::NOT_FOUND.into_response();
    };
    match tokio::task::spawn_blocking(move || std::fs::read(path)).await {
        Ok(Ok(bytes)) => (
            [
                (header::CONTENT_TYPE, content_type(&name)),
                (header::CACHE_CONTROL, "no-store"),
                (header::CONTENT_SECURITY_POLICY, MEDIA_POLICY),
                (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
                (header::ACCESS_CONTROL_ALLOW_ORIGIN, "*"),
            ],
            bytes,
        )
            .into_response(),
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}

/// The sounds of a card side, as the card data has them.
pub(crate) fn sounds_json(sounds: &[Sound]) -> Vec<Value> {
    sounds
        .iter()
        .map(|sound| match sound {
            Sound::File(name) => json!({ "kind": "file", "name": name }),
            Sound::Speech { text, language } => json!({ "kind": "speech", "text": text, "language": language }),
        })
        .collect()
}

/// An embedded file. Files below `assets/` have hashed names, so the browser can keep them. The card document loads
/// them from an opaque origin, so they allow every origin.
fn file(path: &str, bytes: &'static [u8]) -> Response {
    let cache = if path.starts_with("assets/") { "public, max-age=31536000, immutable" } else { "no-store" };
    (
        [
            (header::CONTENT_TYPE, content_type(path)),
            (header::CACHE_CONTROL, cache),
            (header::CONTENT_SECURITY_POLICY, PAGE_POLICY),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
            (header::ACCESS_CONTROL_ALLOW_ORIGIN, "*"),
        ],
        bytes,
    )
        .into_response()
}

fn content_type(path: &str) -> &'static str {
    let extension = path.rsplit_once('.').map(|(_, extension)| extension.to_ascii_lowercase());
    match extension.as_deref() {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("mp3") => "audio/mpeg",
        Some("ogg" | "oga" | "opus") => "audio/ogg",
        Some("wav") => "audio/wav",
        Some("m4a") => "audio/mp4",
        Some("mp4") => "video/mp4",
        Some("webm") => "video/webm",
        Some("ttf") => "font/ttf",
        Some("otf") => "font/otf",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        _ => "application/octet-stream",
    }
}

impl Shared {
    /// Runs the pipeline and returns the JSON body of `/api/preview`.
    fn refresh(&self) -> String {
        let _pipeline = self.pipeline.lock().unwrap_or_else(PoisonError::into_inner);
        let mut diagnostics = Diagnostics::default();
        let loaded = load(&self.deck, self.options, &mut diagnostics).and_then(|mut loaded| {
            let media = media_map(&loaded);
            match render(&mut loaded, &Selection::All) {
                Ok(rows) => Some((rows, media)),
                Err(message) => {
                    diagnostics.push(Diagnostic::error("preview::render", message));
                    None
                },
            }
        });
        let mut snapshot = self.snapshot.lock().unwrap_or_else(PoisonError::into_inner);
        snapshot.generation = generation();
        let stale = loaded.is_none();
        if let Some((rows, media)) = loaded {
            snapshot.rows = rows.iter().enumerate().map(|(index, row)| row_json(index, row)).collect();
            snapshot.cards = rows
                .into_iter()
                .map(|row| Card {
                    css: row.css,
                    ord: row.ord,
                    rendered: row.rendered.ok(),
                })
                .collect();
            snapshot.media = media;
        }
        json_text(&json!({
            "generation": snapshot.generation,
            "stale": stale,
            "diagnostics": diagnostics.iter().map(Diagnostic::to_json).collect::<Vec<_>>(),
            "rows": snapshot.rows,
        }))
    }
}

fn generation() -> String {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).expect("random bytes");
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Loads the project, reads `deck.lock`, and makes the package with the lock file in memory. It reads no media
/// files. It returns `None` if the project or the package fails.
pub(crate) fn load(deck: &Path, options: project::Options, diagnostics: &mut Diagnostics) -> Option<Loaded> {
    let checked = check(deck, options, diagnostics, &mut Vec::new())?;
    missing_media(&checked.rendered, false, diagnostics);
    let mut lock = lock::read(&checked.project.root.join("deck.lock"), diagnostics)?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |time| time.as_secs() as i64);
    let package =
        package::package_with(&checked.project, &checked.files, &checked.rendered, &mut lock, now, false, diagnostics)?;
    Some(Loaded { checked, lock, package })
}

/// The media files of the render, by Anki name.
pub(crate) fn media_map(loaded: &Loaded) -> HashMap<String, PathBuf> {
    let root = &loaded.checked.project.root;
    loaded.checked.rendered.media.iter().map(|file| (file.name.clone(), root.join(&file.path))).collect()
}

/// Renders the cards of `selection`: content notes in package order, which is the new-card position in Anki, then
/// the sample notes.
/// It removes the other notes from the package. It returns Anki's message if Anki rejects the package.
pub(crate) fn render(loaded: &mut Loaded, selection: &Selection) -> std::result::Result<Vec<Row>, String> {
    let checked = &loaded.checked;
    let project = &checked.project;
    let guid = |notetype: &str, id: &str| lock::guid(&loaded.lock.namespace, notetype, id);
    let samples: Vec<&String> = match selection {
        Selection::All => {
            let used: HashSet<&str> = checked.rendered.notes.iter().map(|note| note.notetype.as_str()).collect();
            project.notetypes.keys().filter(|key| !used.contains(key.as_str())).collect()
        },
        Selection::Entry(_) => Vec::new(),
        Selection::Sample(key) => project.notetypes.keys().filter(|candidate| candidate == key).collect(),
    };
    let wanted: Option<HashSet<String>> = match selection {
        Selection::All => None,
        Selection::Entry(id) => Some(
            checked
                .rendered
                .notes
                .iter()
                .filter(|note| note.id == *id)
                .map(|note| guid(&note.notetype, &note.id))
                .collect(),
        ),
        Selection::Sample(_) => Some(HashSet::new()),
    };
    if let Some(wanted) = &wanted {
        loaded.package.notes.retain(|note| wanted.contains(&note.guid));
    }
    let sample_ids: Vec<i64> = samples.iter().map(|key| loaded.lock.notetypes[*key].id).collect();
    let rendered = janky_anki::render(&loaded.package, &sample_ids).map_err(|error| error.0)?;

    let entries: HashMap<&str, (&str, String, Option<usize>)> = checked
        .files
        .iter()
        .flat_map(|file| {
            file.entries.iter().map(move |entry| {
                let index = entry.pointer.strip_prefix("/entries/").and_then(|index| index.parse().ok());
                (entry.id.as_str(), (file.path.as_str(), file.deck.join("."), index))
            })
        })
        .collect();
    let notes: HashMap<String, (&str, &str)> = checked
        .rendered
        .notes
        .iter()
        .map(|note| (guid(&note.notetype, &note.id), (note.notetype.as_str(), note.id.as_str())))
        .collect();
    let decks: HashMap<i64, &str> = loaded.package.decks.iter().map(|deck| (deck.id, deck.name.as_str())).collect();

    let mut content = Vec::new();
    for (note, cards) in loaded.package.notes.iter().zip(rendered.notes) {
        let (notetype, id) = notes[&note.guid];
        let (file, deck, index) = &entries[id];
        let entry = RowEntry {
            id: id.to_string(),
            file: file.to_string(),
            index: *index,
        };
        let deck = (deck.clone(), decks[&note.deck].to_string());
        content.push((notetype, Some(entry), Some(deck), cards));
    }

    let mut rows = Vec::new();
    let all = content
        .into_iter()
        .chain(samples.iter().zip(rendered.samples).map(|(key, cards)| (key.as_str(), None, None, cards)));
    for (notetype, entry, deck, rendered_cards) in all {
        let file = &project.notetypes[notetype].file;
        let css = &checked.rendered.notetypes[notetype].css;
        for (ord, ((card_key, card), rendered)) in file.cards.iter().zip(rendered_cards).enumerate() {
            rows.push(Row {
                notetype: notetype.to_string(),
                notetype_name: file.name.clone(),
                entry: entry.as_ref().map(|entry: &RowEntry| RowEntry {
                    id: entry.id.clone(),
                    file: entry.file.clone(),
                    index: entry.index,
                }),
                deck: deck.clone(),
                card_key: card_key.clone(),
                card_name: card.name.clone(),
                ord,
                css: css.clone(),
                rendered,
            });
        }
    }
    Ok(rows)
}

/// The JSON of one row of `/api/preview`.
fn row_json(index: usize, row: &Row) -> Value {
    json!({
        "row": index,
        "notetype": { "key": row.notetype, "name": row.notetype_name },
        "entry": row.entry.as_ref().map(|entry| json!({ "id": entry.id, "file": entry.file, "index": entry.index })),
        "deck": row.deck.as_ref().map(|(key, full_name)| json!({ "key": key, "full_name": full_name })),
        "card": { "key": row.card_key, "name": row.card_name, "ord": row.ord },
        "empty": row.rendered.as_ref().is_ok_and(|card| card.is_empty),
        "error": row.rendered.as_ref().err(),
    })
}
