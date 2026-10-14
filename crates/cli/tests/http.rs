use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use janky_cli::cli::Checked;
use janky_cli::diagnostic::Diagnostics;
use janky_cli::runner::{self, Outcome};

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

#[derive(Debug, Clone)]
struct Seen {
    head: String,
    body: Vec<u8>,
    at: Instant,
}

struct Server {
    base: String,
    seen: Arc<Mutex<Vec<Seen>>>,
}

fn serve(responses: Vec<(u64, String)>) -> Server {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let seen: Arc<Mutex<Vec<Seen>>> = Arc::default();
    let record = seen.clone();
    let responses = Arc::new(responses);
    let next = Arc::new(AtomicUsize::new(0));
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { return };
            let (record, responses, next) = (record.clone(), responses.clone(), next.clone());
            std::thread::spawn(move || {
                let mut data = Vec::new();
                let mut buffer = [0u8; 4096];
                let end = loop {
                    if let Some(end) = data.windows(4).position(|window| window == b"\r\n\r\n") {
                        break end + 4;
                    }
                    let Ok(read) = stream.read(&mut buffer) else { return };
                    if read == 0 {
                        return;
                    }
                    data.extend_from_slice(&buffer[..read]);
                };
                let head = String::from_utf8_lossy(&data[..end]).to_string();
                let length = head
                    .lines()
                    .find_map(|line| {
                        line.to_ascii_lowercase().strip_prefix("content-length:").map(|value| value.trim().to_string())
                    })
                    .and_then(|value| value.parse::<usize>().ok())
                    .unwrap_or(0);
                while data.len() < end + length {
                    let Ok(read) = stream.read(&mut buffer) else { return };
                    if read == 0 {
                        break;
                    }
                    data.extend_from_slice(&buffer[..read]);
                }
                record.lock().unwrap().push(Seen {
                    head,
                    body: data[end..].to_vec(),
                    at: Instant::now(),
                });
                let index = next.fetch_add(1, Ordering::SeqCst);
                let (delay, response) = responses.get(index).cloned().unwrap_or((0, reply(500, "", "")));
                std::thread::sleep(Duration::from_millis(delay));
                let _ = stream.write_all(response.as_bytes());
            });
        }
    });
    Server { base, seen }
}

fn reply(status: u16, headers: &str, body: &str) -> String {
    format!("HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n{headers}\r\n{body}", body.len())
}

fn project(url: &str, http: &str) -> (tempfile::TempDir, PathBuf) {
    let work = tempfile::tempdir().unwrap();
    let root = work.path().join("generate-test");
    copy(&data().join("generate-test"), &root);
    let plugin = format!(
        "local function plan(job) return {{ ext = 'ogg' }} end\n\
         local function generate(job)\n\
           local r = janky.http.request {{ method = 'POST', url = '{url}', headers = {{ ['content-type'] = 'text/plain' }}, body = job.input }}\n\
           if r.status ~= 200 then return nil, 'HTTP ' .. r.status end\n\
           return {{ data = r.body, ext = 'ogg' }}\n\
         end\n\
         return {{ id = 'voice', api = 1, generates = 'media', variables = {{ speed = {{ type = 'int' }}, voice = {{ type = 'string' }} }}, \
         generate = generate, plan = plan }}\n"
    );
    std::fs::write(root.join("plugins/voice.lua"), plugin).unwrap();
    let deck = std::fs::read_to_string(root.join("deck.toml")).unwrap();
    std::fs::write(root.join("deck.toml"), format!("{deck}{http}")).unwrap();
    (work, root)
}

fn run(root: &Path) -> (Vec<(janky_cli::generate::Job, Outcome)>, Checked) {
    let mut diagnostics = Diagnostics::default();
    let checked = janky_cli::cli::check(&root.join("deck.toml"), Default::default(), &mut diagnostics, &mut Vec::new());
    assert_eq!(diagnostics.to_json_lines(), "");
    let checked = checked.unwrap();
    let client = Arc::new(janky_cli::http::Client::new(&checked.project, Default::default()).unwrap());
    let jobs = checked.rendered.jobs.iter().filter(|job| job.instance == "voice").cloned().collect();
    let limits = runner::limits(&checked.project);
    (runner::run(&checked.plugins, root, jobs, &limits, client, |_| {}, |_, _| true), checked)
}

const FAST: &str = "retry = { base_delay_ms = 10, max_delay_ms = 50 }\n";

#[test]
fn retries_server_errors_and_returns_redirects() {
    let server = serve(vec![
        (0, reply(503, "", "")),
        (0, reply(503, "", "")),
        (0, reply(200, "", "audio")),
        (0, reply(302, "Location: https://elsewhere.example/\r\n", "")),
    ]);
    let (_work, root) = project(
        &format!("{}/tts", server.base),
        &format!("\n[plugins.voice.http]\nallowed_hosts = [\"127.0.0.1\"]\n{FAST}"),
    );
    let (results, _) = run(&root);
    assert!(matches!(&results[0].1, Outcome::Stored { .. }), "{results:?}");
    assert!(matches!(&results[1].1, Outcome::Refused(message) if message == "HTTP 302"), "{results:?}");
    let seen = server.seen.lock().unwrap();
    assert_eq!(seen.len(), 4);
    assert!(seen[0].head.starts_with("POST /tts HTTP/1.1"), "{}", seen[0].head);
    assert!(seen[0].head.to_ascii_lowercase().contains("content-type: text/plain"));
    assert_eq!(seen[0].body, "☿".as_bytes());
}

#[test]
fn retry_after_is_followed_up_to_the_maximum_delay() {
    let server = serve(vec![
        (0, reply(429, "Retry-After: 5\r\n", "")),
        (0, reply(200, "", "a")),
        (0, reply(200, "", "b")),
    ]);
    let http =
        "\n[plugins.voice.http]\nallowed_hosts = [\"127.0.0.1\"]\nretry = { base_delay_ms = 10, max_delay_ms = 300 }\n";
    let (_work, root) = project(&server.base, http);
    let start = Instant::now();
    let (results, _) = run(&root);
    assert!(results.iter().all(|(_, outcome)| matches!(outcome, Outcome::Stored { .. })), "{results:?}");
    let seen = server.seen.lock().unwrap();
    assert!(seen[1].at - seen[0].at < Duration::from_millis(250), "the other job waited for the retry");
    let gap = seen[2].at - seen[0].at;
    assert!(gap >= Duration::from_millis(250) && start.elapsed() < Duration::from_secs(3), "{gap:?}");
}

#[test]
fn hosts_outside_the_allowlist_get_no_request() {
    let server = serve(Vec::new());
    let local = server.base.replace("127.0.0.1", "localhost");
    let (_work, root) = project(&local, "\n[plugins.voice.http]\nallowed_hosts = [\"127.0.0.1\"]\n");
    let (results, _) = run(&root);
    let Outcome::Failed(failure) = &results[0].1 else { panic!("{results:?}") };
    assert!(failure.message.contains("the host `localhost` is not in `allowed_hosts`"), "{}", failure.message);

    let (_work, root) = project(&server.base, "");
    let (results, _) = run(&root);
    let Outcome::Failed(failure) = &results[0].1 else { panic!("{results:?}") };
    assert!(failure.message.contains("has no `[plugins.voice.http]` table"), "{}", failure.message);
    assert!(server.seen.lock().unwrap().is_empty());
}

#[test]
fn concurrency_and_rate_limit_space_the_requests() {
    for (settings, overlap) in [("max_concurrency = 2\n", true), ("max_concurrency = 1\n", false)] {
        let server = serve(vec![(300, reply(200, "", "a")), (300, reply(200, "", "b"))]);
        let http = format!("\n[plugins.voice.http]\nallowed_hosts = [\"127.0.0.1\"]\n{settings}");
        let (_work, root) = project(&server.base, &http);
        let (results, _) = run(&root);
        assert!(results.iter().all(|(_, outcome)| matches!(outcome, Outcome::Stored { .. })), "{results:?}");
        let seen = server.seen.lock().unwrap();
        let gap = seen[1].at - seen[0].at;
        assert_eq!(gap < Duration::from_millis(150), overlap, "{settings}: {gap:?}");
    }
    let server = serve(vec![(0, reply(200, "", "a")), (0, reply(200, "", "b"))]);
    let http = "\n[plugins.voice.http]\nallowed_hosts = [\"127.0.0.1\"]\nmax_concurrency = 2\nrate_limit = { count = 4, unit = \"second\" }\n";
    let (_work, root) = project(&server.base, http);
    run(&root);
    let seen = server.seen.lock().unwrap();
    assert!(seen[1].at - seen[0].at >= Duration::from_millis(200), "{:?}", seen[1].at - seen[0].at);
}

#[test]
fn large_responses_are_refused() {
    let head = format!("HTTP/1.1 200 X\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", 40 * 1024 * 1024);
    let server = serve(vec![(0, head)]);
    let (_work, root) = project(&server.base, "\n[plugins.voice.http]\nallowed_hosts = [\"127.0.0.1\"]\n");
    let (results, _) = run(&root);
    let Outcome::Failed(failure) = &results[0].1 else { panic!("{results:?}") };
    assert!(failure.message.contains("larger than"), "{}", failure.message);
}

#[test]
fn secrets_are_put_in_when_the_request_is_sent() {
    let server = serve(vec![
        (0, reply(200, "", "a")),
        (0, reply(200, "", "echo: http-key-0123456789")),
    ]);
    let (_work, root) = project(
        &server.base,
        "\n[plugins.voice.http]\nallowed_hosts = [\"127.0.0.1\"]\n\n[plugins.voice.secrets]\napi_key = { file = \"key.txt\" }\n",
    );
    let source = std::fs::read_to_string(root.join("plugins/voice.lua")).unwrap();
    let source = source
        .replace("['content-type'] = 'text/plain'", "authorization = janky.secret('api_key', 'Bearer ')")
        .replace("id = 'voice', api = 1,", "id = 'voice', api = 1, secrets = { 'api_key' },");
    std::fs::write(root.join("plugins/voice.lua"), source).unwrap();
    std::fs::write(root.join("key.txt"), "http-key-0123456789\n").unwrap();
    let mut diagnostics = Diagnostics::default();
    let checked =
        janky_cli::cli::check(&root.join("deck.toml"), Default::default(), &mut diagnostics, &mut Vec::new()).unwrap();
    assert_eq!(diagnostics.to_json_lines(), "");
    let secrets = janky_cli::secrets::resolve(&checked.project, &checked.plugins, &mut diagnostics);
    let client = Arc::new(janky_cli::http::Client::new(&checked.project, secrets).unwrap());
    let jobs = checked.rendered.jobs.iter().filter(|job| job.instance == "voice").cloned().collect();
    let results =
        runner::run(&checked.plugins, &root, jobs, &runner::limits(&checked.project), client, |_| {}, |_, _| true);
    assert!(matches!(&results[0].1, Outcome::Stored { .. }), "{results:?}");
    let Outcome::Failed(failure) = &results[1].1 else { panic!("{results:?}") };
    assert!(failure.message.contains("sent the secret `api_key` back"), "{}", failure.message);
    let seen = server.seen.lock().unwrap();
    assert!(
        seen[0].head.to_ascii_lowercase().contains("authorization: bearer http-key-0123456789"),
        "{}",
        seen[0].head
    );
}

fn download_plugin(root: &Path, url: &str, headers: &str) {
    let plugin = format!(
        "local function plan(job) return {{ ext = 'ogg' }} end\n\
         local function generate(job)\n\
           local file = job.input == '☿' and 'one.bin' or 'two.bin'\n\
           local r = janky.http.download {{ url = '{url}/dump', headers = {{ {headers} }}, file = file }}\n\
           return {{ data = r.status .. ' ' .. r.size, ext = 'ogg' }}\n\
         end\n\
         return {{ id = 'voice', api = 1, generates = 'media', secrets = {{ 'api_key' }}, \
         variables = {{ speed = {{ type = 'int' }}, voice = {{ type = 'string' }} }}, generate = generate, plan = plan }}\n"
    );
    std::fs::write(root.join("plugins/voice.lua"), plugin).unwrap();
    std::fs::write(root.join("key.txt"), "http-key-0123456789\n").unwrap();
}

fn run_with_secrets(root: &Path) -> Vec<(janky_cli::generate::Job, Outcome)> {
    let mut diagnostics = Diagnostics::default();
    let checked =
        janky_cli::cli::check(&root.join("deck.toml"), Default::default(), &mut diagnostics, &mut Vec::new()).unwrap();
    let secrets = janky_cli::secrets::resolve(&checked.project, &checked.plugins, &mut diagnostics);
    assert!(diagnostics.is_empty(), "{}", diagnostics.to_json_lines());
    let client = Arc::new(janky_cli::http::Client::new(&checked.project, secrets).unwrap());
    let jobs = checked.rendered.jobs.iter().filter(|job| job.instance == "voice").cloned().collect();
    let limits = runner::limits(&checked.project);
    runner::run(
        &checked.plugins,
        root,
        jobs,
        &limits,
        client,
        |_| {},
        |_, outcome| !matches!(outcome, Outcome::Unauthorised { .. }),
    )
}

fn files(root: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(root.join(".janky/cache/plugins/voice/files"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

fn stored(root: &Path, outcome: &Outcome) -> String {
    let Outcome::Stored { file, .. } = outcome else { panic!("{outcome:?}") };
    std::fs::read_to_string(root.join(".janky/cache/media").join(file)).unwrap()
}

const SECRETS: &str = "\n[plugins.voice.http]\nallowed_hosts = [\"127.0.0.1\"]\nretry = { base_delay_ms = 10, max_delay_ms = 50 }\n\n[plugins.voice.secrets]\napi_key = { file = \"key.txt\" }\n";

#[test]
fn downloads_stream_large_bodies_to_a_file() {
    let large = "a".repeat(janky_cli::http::BODY_LIMIT + 1);
    let server = serve(vec![
        (0, reply(503, "", "")),
        (0, reply(200, "", &large)),
        (0, reply(404, "", "missing")),
    ]);
    let (_work, root) = project(&server.base, SECRETS);
    download_plugin(&root, &server.base, "");
    let results = run_with_secrets(&root);
    let mut data: Vec<(String, String)> =
        results.iter().map(|(job, outcome)| (stored(&root, outcome), job.input_text())).collect();
    data.sort();
    assert_eq!(data[0].0, format!("200 {}", large.len()));
    assert_eq!(data[1].0, "404 0");
    let file = if data[0].1 == "☿" { "one.bin" } else { "two.bin" };
    assert_eq!(files(&root), [file]);
    assert_eq!(
        std::fs::metadata(root.join(".janky/cache/plugins/voice/files").join(file)).unwrap().len(),
        large.len() as u64
    );
    assert_eq!(server.seen.lock().unwrap().len(), 3);
}

#[test]
fn downloads_with_a_secret_or_too_large_leave_no_file() {
    let server = serve(vec![
        (0, reply(200, "", "data http-key-0123456789 data")),
        (0, "HTTP/1.1 200 X\r\nContent-Length: 200000000\r\nConnection: close\r\n\r\n".to_string()),
    ]);
    let (_work, root) = project(&server.base, SECRETS);
    download_plugin(&root, &server.base, "authorization = janky.secret('api_key')");
    let results = run_with_secrets(&root);
    let messages: Vec<String> = results
        .iter()
        .map(|(_, outcome)| match outcome {
            Outcome::Failed(failure) => failure.message.clone(),
            other => format!("{other:?}"),
        })
        .collect();
    assert!(messages[0].contains("sent the secret `api_key` back"), "{messages:?}");
    assert!(messages[1].contains("the download is larger than 134217728 bytes"), "{messages:?}");
    assert!(files(&root).is_empty(), "{:?}", files(&root));
}

fn prepare_plugin(root: &Path, url: &str) {
    let plugin = format!(
        "local function prepare()\n\
           local r = janky.http.request {{ url = '{url}/index', headers = {{ authorization = janky.secret('api_key') }} }}\n\
           if r.status ~= 200 then error('HTTP ' .. r.status) end\n\
           janky.cache.put('index', r.body)\n\
         end\n\
         local function generate(job) return {{ data = janky.cache.get('index') .. ' ' .. job.input, ext = 'ogg' }} end\n\
         return {{ id = 'voice', api = 1, generates = 'media', secrets = {{ 'api_key' }}, \
         variables = {{ speed = {{ type = 'int' }}, voice = {{ type = 'string' }} }}, generate = generate, prepare = prepare }}\n"
    );
    std::fs::write(root.join("plugins/voice.lua"), plugin).unwrap();
    std::fs::write(root.join("key.txt"), "http-key-0123456789\n").unwrap();
}

#[test]
fn prepare_can_retry_requests() {
    let server = serve(vec![(0, reply(503, "", "")), (0, reply(200, "", "index"))]);
    let (_work, root) = project(&server.base, SECRETS);
    prepare_plugin(&root, &server.base);
    let results = run_with_secrets(&root);
    let mut data: Vec<String> = results.iter().map(|(_, outcome)| stored(&root, outcome)).collect();
    data.sort();
    assert_eq!(data, ["index ☿", "index 🜚"]);
    assert_eq!(server.seen.lock().unwrap().len(), 2);
}

#[test]
fn a_refused_key_in_prepare_stops_the_run() {
    let server = serve(vec![(0, reply(401, "", "bad key"))]);
    let (_work, root) = project(&server.base, SECRETS);
    prepare_plugin(&root, &server.base);
    let results = run_with_secrets(&root);
    let Outcome::Unauthorised { message, secrets } = &results[0].1 else {
        panic!("{results:?}")
    };
    assert!(message.contains("`prepare` failed") && message.contains("HTTP 401"), "{message}");
    assert_eq!(secrets, &["api_key"]);
    assert!(matches!(results[1].1, Outcome::Skipped), "{results:?}");
}

#[test]
fn compressed_or_split_echoes_do_not_reach_the_plugin() {
    let split = "X-Echo: secret-part\r\nX-Echo: other-part\r\n";
    let server = serve(vec![
        (0, reply(200, "Content-Encoding: gzip\r\n", "zz")),
        (0, reply(200, split, "a")),
    ]);
    let (_work, root) = project(
        &server.base,
        "\n[plugins.voice.http]\nallowed_hosts = [\"127.0.0.1\"]\n\n[plugins.voice.secrets]\napi_key = { file = \"key.txt\" }\n",
    );
    let source = std::fs::read_to_string(root.join("plugins/voice.lua")).unwrap();
    let source = source
        .replace("['content-type'] = 'text/plain'", "authorization = janky.secret('api_key')")
        .replace("id = 'voice', api = 1,", "id = 'voice', api = 1, secrets = { 'api_key' },");
    std::fs::write(root.join("plugins/voice.lua"), source).unwrap();
    std::fs::write(root.join("key.txt"), "secret-part, other-part\n").unwrap();
    let mut diagnostics = Diagnostics::default();
    let checked =
        janky_cli::cli::check(&root.join("deck.toml"), Default::default(), &mut diagnostics, &mut Vec::new()).unwrap();
    let secrets = janky_cli::secrets::resolve(&checked.project, &checked.plugins, &mut diagnostics);
    assert!(diagnostics.is_empty(), "{}", diagnostics.to_json_lines());
    let client = Arc::new(janky_cli::http::Client::new(&checked.project, secrets).unwrap());
    let jobs = checked.rendered.jobs.iter().filter(|job| job.instance == "voice").cloned().collect();
    let results =
        runner::run(&checked.plugins, &root, jobs, &runner::limits(&checked.project), client, |_| {}, |_, _| true);
    let messages: Vec<String> = results
        .iter()
        .map(|(_, outcome)| match outcome {
            Outcome::Failed(failure) => failure.message.clone(),
            other => format!("{other:?}"),
        })
        .collect();
    assert!(messages[0].contains("compressed response"), "{messages:?}");
    assert!(messages[1].contains("sent the secret `api_key` back"), "{messages:?}");
}
