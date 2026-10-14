//! The real HTTP client behind `janky.http.request` and `janky.http.download`, used by
//! `janky generate`.
//!
//! [`Client`] implements [`crate::runner::Http`]. Each plugin instance with a
//! `[plugins.<name>.http]` table gets its own limits: allowed hosts, concurrency, a rate limit
//! and retries. An instance without that table has no network access. [`check_url`] decides
//! which URLs a request can go to.
//!
//! The client does not follow redirects, does not use a proxy and does not accept compressed
//! responses. It marks headers that hold a secret as sensitive. It refuses a response that
//! contains a secret of the instance, so the plugin never sees a secret that a server sends
//! back. A download streams a 2xx body to a temporary file and checks it for secrets on the
//! way, so the body never enters Lua.

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use reqwest::Url;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use tokio::sync::Semaphore;
use tokio::time::Instant;

use crate::config::deck::{Http as HttpConfig, RateUnit};
use crate::project::Project;
use crate::runner::{Downloaded, Downloading, HeaderText, Http, Pause, Request, Response, Sending};
use crate::secrets::Secrets;

/// The largest response body that the client reads, in bytes.
pub const BODY_LIMIT: usize = 32 * 1024 * 1024;
/// The largest body that a download writes, in bytes.
pub const DOWNLOAD_LIMIT: u64 = 128 * 1024 * 1024;
/// The most bytes that the downloads of one plugin instance write in one run.
pub const DOWNLOAD_TOTAL_LIMIT: u64 = 512 * 1024 * 1024;
/// The time limit for one attempt of a request, from connect to the last byte.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);
/// The time limit for one attempt of a download, from connect to the last byte.
pub const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(600);
/// The time limit to open a connection.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
/// The longest wait between two requests that a rate limit can ask for.
pub const SPACING_LIMIT: Duration = Duration::from_secs(60);

struct Instance {
    allowed: Vec<String>,
    slots: Arc<Semaphore>,
    spacing: Option<Duration>,
    next: Arc<Mutex<Option<Instant>>>,
    attempts: u32,
    base_delay: Duration,
    max_delay: Duration,
    downloaded: AtomicU64,
}

/// An HTTP client with the network limits and the secrets of each plugin instance.
pub struct Client {
    client: reqwest::Client,
    instances: HashMap<String, Arc<Instance>>,
    secrets: Arc<Secrets>,
}

impl Client {
    /// Makes the client from the `[plugins.<name>.http]` tables of the project.
    ///
    /// It fails when the HTTP client cannot start, or when a rate limit allows fewer than one
    /// request per [`SPACING_LIMIT`].
    pub fn new(project: &Project, secrets: Secrets) -> Result<Self, String> {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(REQUEST_TIMEOUT)
            .connect_timeout(CONNECT_TIMEOUT)
            .no_proxy()
            .build()
            .map_err(|error| format!("cannot start the HTTP client: {error}"))?;
        let mut instances = HashMap::new();
        for (name, instance) in &project.deck.plugins {
            let Some(http) = &instance.http else { continue };
            let settings = instance_for(http);
            if settings.spacing.is_some_and(|spacing| spacing > SPACING_LIMIT) {
                return Err(format!(
                    "`[plugins.{name}.http]`: the rate limit allows fewer than one request per {} seconds",
                    SPACING_LIMIT.as_secs()
                ));
            }
            instances.insert(name.clone(), Arc::new(settings));
        }
        Ok(Self {
            client,
            instances,
            secrets: Arc::new(secrets),
        })
    }
}

fn instance_for(http: &HttpConfig) -> Instance {
    let spacing = http.rate_limit.as_ref().map(|limit| {
        let unit = match limit.unit {
            RateUnit::Second => 1,
            RateUnit::Minute => 60,
            RateUnit::Hour => 3600,
        };
        Duration::from_secs(unit) / limit.count.max(1)
    });
    Instance {
        allowed: http.allowed_hosts.clone(),
        slots: Arc::new(Semaphore::new(http.max_concurrency.max(1) as usize)),
        spacing,
        next: Arc::default(),
        attempts: http.retry.max_attempts.max(1),
        base_delay: Duration::from_millis(http.retry.base_delay_ms),
        max_delay: Duration::from_millis(http.retry.max_delay_ms),
        downloaded: AtomicU64::new(0),
    }
}

impl Http for Client {
    /// Sends a request for a plugin instance, with retries.
    ///
    /// The request fails at once when the instance has no `http` table, or the URL fails
    /// [`check_url`]. It also fails at once when the method or a header is not valid, or a
    /// secret is not available. Each attempt waits for the rate limit and for a free
    /// concurrency slot.
    ///
    /// A 429 or a 5xx status other than 501 is retried. A transport error is retried when the
    /// connection failed, or when the method is not `POST` or `PATCH`. The wait is
    /// `Retry-After` in seconds when the server sends it, otherwise an exponential backoff with
    /// jitter. Both are at most the maximum delay. The wait uses `pause` and does not hold a
    /// concurrency slot. After the last attempt, a retryable status comes back as a normal
    /// response.
    ///
    /// Response header names are lower-case, and repeated headers are joined with `, `. When
    /// the request fails after it was sent, the error names the host.
    fn send(&self, instance: &str, request: Request, pause: Pause) -> Sending {
        let exchange = self.exchange(instance, request, pause, None);
        Box::pin(async move {
            let received = exchange.await?;
            Ok(Response {
                status: received.status,
                headers: received.headers,
                body: received.body,
            })
        })
    }

    /// Sends a request like [`Http::send`], and writes a 2xx body to `file` instead of giving
    /// it back.
    ///
    /// The body goes to a temporary file next to `file`, which janky renames when the body is
    /// complete. A body larger than [`DOWNLOAD_LIMIT`], a body that holds a secret value of the
    /// instance, a broken or cancelled transfer, or a directory of `file` that moved or became a
    /// link during the transfer leaves no file. Another status writes
    /// nothing. Each attempt has [`DOWNLOAD_TIMEOUT`]. All downloads of an instance together
    /// write at most [`DOWNLOAD_TOTAL_LIMIT`] in one run.
    fn download(&self, instance: &str, request: Request, file: PathBuf, pause: Pause) -> Downloading {
        let exchange = self.exchange(instance, request, pause, Some(file));
        Box::pin(async move {
            let received = exchange.await?;
            Ok(Downloaded {
                status: received.status,
                headers: received.headers,
                size: received.size,
            })
        })
    }
}

struct Received {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    size: u64,
}

type Exchanging = std::pin::Pin<Box<dyn std::future::Future<Output = Result<Received, String>> + Send>>;

impl Client {
    fn exchange(&self, instance: &str, request: Request, pause: Pause, target: Option<PathBuf>) -> Exchanging {
        let client = self.client.clone();
        let settings = self.instances.get(instance).cloned();
        let secrets = self.secrets.clone();
        let instance = instance.to_string();
        Box::pin(async move {
            let Some(settings) = settings else {
                return Err(format!(
                    "`{instance}` has no `[plugins.{instance}.http]` table, so it has no network access"
                ));
            };
            let url = check_url(&request.url, &settings.allowed)?;
            let method = reqwest::Method::from_bytes(request.method.as_bytes()).map_err(|error| error.to_string())?;
            let mut headers = HeaderMap::new();
            for (name, value) in &request.headers {
                let name = HeaderName::from_bytes(name.as_bytes())
                    .map_err(|_| format!("`{name}` is not a valid header name"))?;
                let value = match value {
                    HeaderText::Text(text) => HeaderValue::from_str(text)
                        .map_err(|_| format!("the header `{name}` has a value that is not valid"))?,
                    HeaderText::Secret { name: secret, prefix } => {
                        let value = secrets
                            .get(&instance, secret)
                            .ok_or_else(|| format!("the secret `{secret}` is not available"))?;
                        let mut value = HeaderValue::from_str(&format!("{prefix}{value}"))
                            .map_err(|_| format!("the secret `{secret}` cannot be a header value"))?;
                        value.set_sensitive(true);
                        value
                    },
                };
                headers.append(name, value);
            }
            let forms = secret_forms(&secrets, &instance);
            let host = url.host_str().unwrap_or_default().to_string();
            let mut attempt = 1;
            loop {
                wait_for_turn(&settings).await;
                let sent = {
                    let _slot = settings.slots.acquire().await.map_err(|error| error.to_string())?;
                    let mut request =
                        client.request(method.clone(), url.clone()).headers(headers.clone()).body(request.body.clone());
                    if target.is_some() {
                        request = request.timeout(DOWNLOAD_TIMEOUT);
                    }
                    match request.send().await {
                        Ok(response) => receive(response, target.as_deref(), &forms, &settings.downloaded).await,
                        Err(error) => Err(Failure::Transport {
                            connect: error.is_connect(),
                            message: describe(error),
                        }),
                    }
                };
                let retry = match &sent {
                    Ok(received) => retryable(received.status).then(|| retry_after(&received.headers)),
                    Err(Failure::Transport { connect, .. }) if *connect || idempotent(&method) => Some(None),
                    Err(Failure::Transport { .. }) => None,
                    Err(Failure::Final(_)) => None,
                };
                match retry {
                    Some(after) if attempt < settings.attempts => {
                        pause.sleep(delay(&settings, attempt, after)).await;
                        attempt += 1;
                    },
                    _ => {
                        return sent.map_err(|failure| match failure {
                            Failure::Transport { message, .. } => {
                                format!("the request to {host} failed after {attempt} attempts: {message}")
                            },
                            Failure::Final(message) => format!("the request to {host} failed: {message}"),
                        });
                    },
                }
            }
        })
    }
}

enum Failure {
    Transport { connect: bool, message: String },
    Final(String),
}

/// Parses `text` and checks that a request can go to it.
///
/// The URL must use `https`, or `http` for a loopback host. It must not hold a user name or a
/// password. A domain must pass [`host_allowed`], and an IPv4 address must be in `allowed` as
/// written. IPv6 hosts are never allowed. The error is a message for the author.
pub fn check_url(text: &str, allowed: &[String]) -> Result<Url, String> {
    let url = Url::parse(text).map_err(|error| format!("the URL is not valid: {error}"))?;
    if !url.username().is_empty() || url.password().is_some() {
        return Err("a URL cannot hold a user name or a password".into());
    }
    let loopback = match url.host() {
        Some(url::Host::Domain(domain)) => domain == "localhost",
        Some(url::Host::Ipv4(address)) => address.is_loopback(),
        Some(url::Host::Ipv6(address)) => address.is_loopback(),
        None => return Err(format!("`{text}` has no host")),
    };
    match url.scheme() {
        "https" => {},
        "http" if loopback => {},
        "http" => return Err("plain `http://` is allowed only for loopback addresses: use `https://`".into()),
        other => return Err(format!("the scheme `{other}` is not allowed")),
    }
    let permitted = match url.host() {
        Some(url::Host::Domain(domain)) => host_allowed(domain, allowed),
        Some(url::Host::Ipv4(address)) => allowed.iter().any(|entry| *entry == address.to_string()),
        _ => false,
    };
    if !permitted {
        let host = url.host_str().unwrap_or_default();
        return Err(format!("the host `{host}` is not in `allowed_hosts`"));
    }
    Ok(url)
}

/// Whether `domain` matches an entry of `allowed_hosts`.
///
/// An entry matches its exact name. An entry `*.example.com` matches every subdomain of
/// `example.com` at any depth, but not `example.com` itself. A name with a trailing dot never
/// matches.
pub fn host_allowed(domain: &str, allowed: &[String]) -> bool {
    if domain.ends_with('.') || domain.is_empty() {
        return false;
    }
    allowed.iter().any(|entry| match entry.strip_prefix("*.") {
        Some(parent) => {
            domain.len() > parent.len() + 1
                && domain.ends_with(parent)
                && domain[..domain.len() - parent.len()].ends_with('.')
        },
        None => entry == domain,
    })
}

async fn wait_for_turn(settings: &Instance) {
    let Some(spacing) = settings.spacing else { return };
    let at = {
        let mut next = settings.next.lock().expect("rate limit");
        let now = Instant::now();
        let at = next.map_or(now, |next| next.max(now));
        *next = Some(at + spacing);
        at
    };
    tokio::time::sleep_until(at).await;
}

async fn receive(
    mut response: reqwest::Response,
    target: Option<&Path>,
    forms: &[(String, Vec<u8>)],
    downloaded: &AtomicU64,
) -> Result<Received, Failure> {
    let status = response.status().as_u16();
    let mut joined: std::collections::BTreeMap<String, String> = std::collections::BTreeMap::new();
    for (name, value) in response.headers() {
        let value = String::from_utf8_lossy(value.as_bytes());
        joined
            .entry(name.as_str().to_ascii_lowercase())
            .and_modify(|all| {
                all.push_str(", ");
                all.push_str(&value);
            })
            .or_insert_with(|| value.into_owned());
    }
    if joined.get("content-encoding").is_some_and(|encoding| !encoding.trim().eq_ignore_ascii_case("identity")) {
        return Err(Failure::Final("the server sent a compressed response, which janky does not accept".into()));
    }
    let headers: Vec<(String, String)> = joined.into_iter().collect();
    if let Some(name) = echoed_in_headers(forms, &headers) {
        return Err(echo(name));
    }
    let (body, size) = match target {
        None => {
            let body = read(&mut response).await?;
            if let Some((name, _)) = forms.iter().find(|(_, form)| contains(&body, form)) {
                return Err(echo(name));
            }
            let size = body.len() as u64;
            (body, size)
        },
        Some(file) if (200..300).contains(&status) => (Vec::new(), save(&mut response, file, forms, downloaded).await?),
        Some(_) => (Vec::new(), 0),
    };
    Ok(Received { status, headers, body, size })
}

async fn read(response: &mut reqwest::Response) -> Result<Vec<u8>, Failure> {
    if response.content_length().is_some_and(|length| length > BODY_LIMIT as u64) {
        return Err(Failure::Final(format!("the response is larger than {BODY_LIMIT} bytes")));
    }
    let mut body = Vec::new();
    loop {
        match response.chunk().await {
            Ok(Some(chunk)) => {
                if body.len() + chunk.len() > BODY_LIMIT {
                    return Err(Failure::Final(format!("the response is larger than {BODY_LIMIT} bytes")));
                }
                body.extend_from_slice(&chunk);
            },
            Ok(None) => return Ok(body),
            Err(error) => return Err(Failure::Transport { connect: false, message: describe(error) }),
        }
    }
}

async fn save(
    response: &mut reqwest::Response,
    target: &Path,
    forms: &[(String, Vec<u8>)],
    downloaded: &AtomicU64,
) -> Result<u64, Failure> {
    if response.content_length().is_some_and(|length| length > DOWNLOAD_LIMIT) {
        return Err(Failure::Final(format!("the download is larger than {DOWNLOAD_LIMIT} bytes")));
    }
    let (temporary, mut file) = crate::cache::temporary(target).map_err(Failure::Final)?;
    let mut partial = Partial(Some(temporary.clone()));
    let keep = forms.iter().map(|(_, form)| form.len()).max().unwrap_or(1).saturating_sub(1);
    let mut tail = Vec::new();
    let mut size = 0;
    loop {
        match response.chunk().await {
            Ok(Some(chunk)) => {
                size += chunk.len() as u64;
                if size > DOWNLOAD_LIMIT {
                    return Err(Failure::Final(format!("the download is larger than {DOWNLOAD_LIMIT} bytes")));
                }
                if downloaded.fetch_add(chunk.len() as u64, Ordering::Relaxed) + chunk.len() as u64
                    > DOWNLOAD_TOTAL_LIMIT
                {
                    return Err(Failure::Final(format!(
                        "the downloads of this instance wrote more than {DOWNLOAD_TOTAL_LIMIT} bytes in this run"
                    )));
                }
                tail.extend_from_slice(&chunk);
                if let Some((name, _)) = forms.iter().find(|(_, form)| contains(&tail, form)) {
                    return Err(echo(name));
                }
                tail.drain(..tail.len().saturating_sub(keep));
                file.write_all(&chunk)
                    .map_err(|error| Failure::Final(format!("cannot write the download: {error}")))?;
            },
            Ok(None) => break,
            Err(error) => return Err(Failure::Transport { connect: false, message: describe(error) }),
        }
    }
    file.sync_all().map_err(|error| Failure::Final(format!("cannot write the download: {error}")))?;
    let dir = target.parent().unwrap_or(Path::new("."));
    if dir.canonicalize().ok().as_deref() != Some(dir) {
        return Err(Failure::Final("the file area changed during the download".into()));
    }
    std::fs::rename(&temporary, target)
        .map_err(|error| Failure::Final(format!("cannot write the download: {error}")))?;
    partial.0 = None;
    Ok(size)
}

/// A temporary download file that is removed when the download stops before the rename,
/// also when the job is cancelled.
struct Partial(Option<PathBuf>);

impl Drop for Partial {
    fn drop(&mut self) {
        if let Some(path) = self.0.take() {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// The byte forms of each secret value of `instance` that a response must not hold: the
/// value, and the value as it appears inside a JSON string.
fn secret_forms(secrets: &Secrets, instance: &str) -> Vec<(String, Vec<u8>)> {
    let mut forms = Vec::new();
    for (name, value) in secrets.values(instance) {
        let escaped = serde_json::to_string(value).unwrap_or_default();
        forms.push((name.clone(), value.as_bytes().to_vec()));
        forms.push((name.clone(), escaped.trim_matches('"').as_bytes().to_vec()));
    }
    forms
}

fn echoed_in_headers<'a>(forms: &'a [(String, Vec<u8>)], headers: &[(String, String)]) -> Option<&'a String> {
    forms.iter().find_map(|(name, form)| {
        headers
            .iter()
            .any(|(header, text)| {
                contains(header.as_bytes(), form)
                    || contains(header.as_bytes(), form.to_ascii_lowercase().as_slice())
                    || contains(text.as_bytes(), form)
            })
            .then_some(name)
    })
}

fn echo(name: &str) -> Failure {
    Failure::Final(format!(
        "the server sent the secret `{name}` back, so janky does not give the response to the plugin"
    ))
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && memchr::memmem::find(haystack, needle).is_some()
}

/// The message of a reqwest error and of each of its causes, joined with `: `, without the
/// URL.
pub(crate) fn describe(error: reqwest::Error) -> String {
    let error = error.without_url();
    let mut message = error.to_string();
    let mut cause = std::error::Error::source(&error);
    while let Some(inner) = cause {
        message.push_str(&format!(": {inner}"));
        cause = inner.source();
    }
    message
}

fn idempotent(method: &reqwest::Method) -> bool {
    !matches!(*method, reqwest::Method::POST | reqwest::Method::PATCH)
}

fn retryable(status: u16) -> bool {
    status == 429 || ((500..=599).contains(&status) && status != 501)
}

fn retry_after(headers: &[(String, String)]) -> Option<Duration> {
    headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("retry-after"))
        .and_then(|(_, value)| value.trim().parse::<u64>().ok())
        .map(|seconds| Duration::from_secs(seconds.min(86400)))
}

fn delay(settings: &Instance, attempt: u32, after: Option<Duration>) -> Duration {
    if let Some(after) = after {
        return after.min(settings.max_delay);
    }
    let factor = 1u32.checked_shl(attempt.saturating_sub(1).min(20)).unwrap_or(u32::MAX);
    let full = settings.base_delay.saturating_mul(factor).min(settings.max_delay);
    let jitter = getrandom::u64().unwrap_or(0) % (full.as_millis() as u64 / 2 + 1);
    full - Duration::from_millis(jitter)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hosts(list: &[&str]) -> Vec<String> {
        list.iter().map(|host| host.to_string()).collect()
    }

    #[test]
    fn hosts_match_exactly_or_below_a_wildcard() {
        let allowed = hosts(&["api.example.com", "*.example.org"]);
        assert!(host_allowed("api.example.com", &allowed));
        assert!(!host_allowed("x.api.example.com", &allowed));
        assert!(!host_allowed("api.example.com.", &allowed));
        assert!(host_allowed("a.example.org", &allowed));
        assert!(host_allowed("a.b.example.org", &allowed));
        assert!(!host_allowed("example.org", &allowed));
        assert!(!host_allowed("badexample.org", &allowed));
        assert!(!host_allowed("evil.com", &allowed));
    }

    #[test]
    fn urls_need_https_an_allowed_host_and_no_user() {
        let allowed = hosts(&["api.example.com", "127.0.0.1", "localhost"]);
        assert!(check_url("https://api.example.com/v1?a=b", &allowed).is_ok());
        assert!(check_url("HTTPS://API.EXAMPLE.COM/", &allowed).is_ok());
        assert!(check_url("http://127.0.0.1:8080/", &allowed).is_ok());
        assert!(check_url("http://localhost/", &allowed).is_ok());
        for (url, message) in [
            ("http://api.example.com/", "loopback"),
            ("ftp://api.example.com/", "scheme"),
            ("https://user:pw@api.example.com/", "user name"),
            ("https://evil.example/", "not in `allowed_hosts`"),
            ("https://api.example.com./", "not in `allowed_hosts`"),
            ("https://[::1]/", "not in `allowed_hosts`"),
            ("not a url", "the URL is not valid"),
        ] {
            let error = check_url(url, &allowed).unwrap_err();
            assert!(error.contains(message), "{url}: {error}");
        }
        assert!(check_url("https://127.0.0.2/", &allowed).is_err());
        assert!(check_url("https://2130706433/", &hosts(&["api.example.com"])).is_err());
    }

    #[test]
    fn delays_grow_and_stay_below_the_maximum() {
        let settings = Instance {
            allowed: Vec::new(),
            slots: Arc::new(Semaphore::new(1)),
            spacing: None,
            next: Arc::default(),
            attempts: 20,
            base_delay: Duration::from_millis(100),
            max_delay: Duration::from_millis(1000),
            downloaded: AtomicU64::new(0),
        };
        for attempt in 1..30 {
            let wait = delay(&settings, attempt, None);
            assert!(wait <= Duration::from_millis(1000), "{attempt}: {wait:?}");
        }
        let first = delay(&settings, 1, None);
        assert!(first >= Duration::from_millis(50) && first <= Duration::from_millis(100), "{first:?}");
        assert_eq!(delay(&settings, 1, Some(Duration::from_secs(90))), Duration::from_millis(1000));
        assert!(retryable(503) && retryable(429) && !retryable(501) && !retryable(404));
        let headers = vec![("Retry-After".to_string(), " 3 ".to_string())];
        assert_eq!(retry_after(&headers), Some(Duration::from_secs(3)));
    }

    #[test]
    fn a_cancelled_download_leaves_no_file() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/dump", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buffer = [0u8; 4096];
            let _ = stream.read(&mut buffer);
            stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 1000\r\n\r\npartial").unwrap();
            std::thread::sleep(Duration::from_millis(500));
        });
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().canonicalize().unwrap().join("dump.zip");
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let outcome = runtime.block_on(async {
            let mut response = reqwest::Client::new().get(&url).send().await.unwrap();
            let downloaded = AtomicU64::new(0);
            tokio::time::timeout(Duration::from_millis(200), save(&mut response, &target, &[], &downloaded)).await
        });
        assert!(outcome.is_err());
        server.join().unwrap();
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }
}
