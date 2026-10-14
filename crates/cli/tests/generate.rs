use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use janky_cli::cli::Checked;
use janky_cli::diagnostic::Diagnostics;
use janky_cli::runner::{self, FakeHttp, HeaderText, Outcome, Response};

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

fn project(voice: Option<&str>) -> (tempfile::TempDir, PathBuf) {
    project_with(voice, "")
}

fn project_with(voice: Option<&str>, deck: &str) -> (tempfile::TempDir, PathBuf) {
    let work = tempfile::tempdir().unwrap();
    let root = work.path().join("generate-test");
    copy(&data().join("generate-test"), &root);
    if let Some(source) = voice {
        std::fs::write(root.join("plugins/voice.lua"), source).unwrap();
    }
    let text = std::fs::read_to_string(root.join("deck.toml")).unwrap();
    std::fs::write(root.join("deck.toml"), format!("{text}{deck}")).unwrap();
    (work, root)
}

fn check(root: &Path) -> Checked {
    let mut diagnostics = Diagnostics::default();
    let checked = janky_cli::cli::check(&root.join("deck.toml"), Default::default(), &mut diagnostics, &mut Vec::new());
    assert_eq!(diagnostics.to_json_lines(), "");
    checked.unwrap()
}

fn voice(generate: &str) -> String {
    format!(
        "local function plan(job) return {{ ext = 'ogg', units = utf8.len(job.input), unit = 'characters' }} end\n\
         local function generate(job, vars)\n{generate}\nend\n\
         return {{ id = 'voice', api = 1, generates = 'media', variables = {{ speed = {{ type = 'int' }}, voice = {{ type = 'string' }} }}, \
         generate = generate, plan = plan }}\n"
    )
}

fn run(
    checked: &Checked,
    root: &Path,
    instance: &str,
    limit: usize,
    http: Arc<FakeHttp>,
) -> Vec<(janky_cli::generate::Job, Outcome)> {
    let jobs = checked.rendered.jobs.iter().filter(|job| job.instance == instance).cloned().collect();
    let limits = HashMap::from([(instance.to_string(), limit)]);
    runner::run(&checked.plugins, root, jobs, &limits, http, |_| {}, |_, _| true)
}

fn ok(status: u16, body: &str, headers: &[(&str, &str)]) -> Result<Response, String> {
    let headers = headers.iter().map(|(name, value)| (name.to_string(), value.to_string())).collect();
    Ok(Response {
        status,
        headers,
        body: body.as_bytes().to_vec(),
    })
}

#[test]
fn pure_lua_generators_fill_the_cache() {
    let (_work, root) = project(None);
    let checked = check(&root);
    let http = Arc::new(FakeHttp::default());
    for instance in ["voice", "plain"] {
        let results = run(&checked, &root, instance, 1, http.clone());
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|(_, outcome)| matches!(outcome, Outcome::Stored { .. })), "{results:?}");
    }
    let after = check(&root);
    assert!(after.rendered.jobs.iter().all(|job| job.exists));
    let plain = after.rendered.jobs.iter().find(|job| job.instance == "plain").unwrap();
    assert!(plain.file.ends_with(".svg"));
    assert_eq!(
        std::fs::read(root.join(".janky/cache/media").join(&plain.file)).unwrap(),
        plain.input_text().as_bytes()
    );
    assert!(http.requests.lock().unwrap().is_empty());
}

#[test]
fn an_empty_input_records_no_job() {
    let (_work, root) = project(None);
    let word = root.join("notetypes/word.toml");
    let text = std::fs::read_to_string(&word).unwrap();
    let field = "\n[fields.empty]\nname = \"Empty\"\nvalue = \"[{{ entry.missing | media('voice') | sound }}][{{ media(' ', 'plain') | sound }}][{{ media('', 'voice').exists }}]\"\n";
    std::fs::write(&word, format!("{text}{field}")).unwrap();
    let checked = check(&root);
    assert!(
        checked.rendered.jobs.iter().all(|job| !job.input_text().trim().is_empty()),
        "{:?}",
        checked.rendered.jobs
    );
    assert_eq!(checked.rendered.jobs.len(), 4);
    assert!(checked.rendered.notes.iter().all(|note| note.fields["empty"] == "[][][False]"));
}

#[test]
fn map_inputs_reach_plan_and_generate() {
    let source = "local function plan(job) return { ext = 'txt', units = job.input.n } end\n\
                  local function generate(job) return { data = job.kind .. ' ' .. janky.json.encode(job.input), ext = 'txt' } end\n\
                  return { id = 'voice', api = 1, generates = 'media', variables = { speed = { type = 'int' }, voice = { type = 'string' } }, \
                  generate = generate, plan = plan }\n";
    let (_work, root) = project(Some(source));
    let word = root.join("notetypes/word.toml");
    let text = std::fs::read_to_string(&word).unwrap();
    let field = "\n[fields.map]\nname = \"Map\"\nvalue = \"{{ media({'word': entry.symbol, 'n': 3.0, 'gone': entry.missing}, 'voice') }}\"\n\
                 \n[fields.same]\nname = \"Same\"\nvalue = \"{{ media({'a': 1}, 'plain') }}{{ media('{\\\"a\\\":1}', 'plain') }}{{ media(entry.symbol, 'voice') | media('plain') }}\"\n";
    std::fs::write(&word, format!("{text}{field}")).unwrap();
    let checked = check(&root);
    let plain: Vec<_> =
        checked.rendered.jobs.iter().filter(|job| job.instance == "plain").map(|job| &job.input).collect();
    assert!(
        plain.contains(&&serde_json::json!({ "a": 1 })) && plain.contains(&&serde_json::json!(r#"{"a":1}"#)),
        "{plain:?}"
    );
    assert!(plain.iter().any(|input| input.as_str().is_some_and(|text| text.ends_with(".txt"))), "{plain:?}");
    let maps: Vec<_> =
        checked.rendered.jobs.iter().filter(|job| job.instance == "voice" && job.input.is_object()).collect();
    assert_eq!(maps.len(), 2);
    assert_eq!(maps[0].input, serde_json::json!({ "n": 3, "word": "☿" }));
    assert_eq!(maps[0].units, Some(3));
    let results = run(&checked, &root, "voice", 1, Arc::new(FakeHttp::default()));
    let file = results
        .iter()
        .find_map(|(job, outcome)| match outcome {
            Outcome::Stored { file, .. } if job.input.is_object() && job.input["word"] == "☿" => Some(file.clone()),
            _ => None,
        })
        .unwrap();
    let data = std::fs::read_to_string(root.join(".janky/cache/media").join(file)).unwrap();
    assert!(data.starts_with("media {"), "{data}");
    assert!(data.contains("\"word\":\"☿\"") && data.contains("\"n\":3") && !data.contains("gone"), "{data}");
}

#[test]
fn plugin_caches_persist_and_are_separate_for_each_instance() {
    let source = voice(
        "local before = janky.cache.get('last')\n\
         janky.cache.put('last', job.input)\n\
         janky.cache.put('table', { word = job.input, n = 2 })\n\
         janky.cache.put('gone', 1)\n\
         janky.cache.put('gone', nil)\n\
         return { data = tostring(before) .. ' ' .. janky.cache.get('table').word .. janky.cache.get('table').n .. ' ' .. tostring(janky.cache.get('gone')), ext = 'ogg' }",
    );
    let (_work, root) = project(Some(&source));
    std::fs::write(
        root.join("plugins/plain.lua"),
        "return { id = 'plain', api = 1, generates = 'media', generate = function(job) return { data = tostring(janky.cache.get('last')), ext = 'svg' } end }\n",
    )
    .unwrap();
    let checked = check(&root);
    let http = Arc::new(FakeHttp::default());
    let mut data = Vec::new();
    for instance in ["voice", "plain"] {
        for (job, outcome) in run(&checked, &root, instance, 1, http.clone()) {
            let Outcome::Stored { file, .. } = outcome else { panic!("{outcome:?}") };
            let text = std::fs::read_to_string(root.join(".janky/cache/media").join(file)).unwrap();
            data.push(format!("{} {}: {text}", job.instance, job.input_text()));
        }
    }
    assert_eq!(
        data,
        [
            "voice ☿: nil ☿2 nil",
            "voice 🜚: ☿ 🜚2 nil",
            "plain ☿: nil",
            "plain 🜚: nil",
        ]
    );
    let voice = janky_cli::plugin_cache::PluginCache::open(&root, "voice").unwrap();
    assert_eq!(voice.get("last").unwrap(), Some(serde_json::json!("🜚")));
    assert!(root.join(".janky/cache/plugins/voice/cache.sqlite").is_file());
    let plain = janky_cli::plugin_cache::PluginCache::open(&root, "plain").unwrap();
    assert_eq!(plain.get("last").unwrap(), None);
}

#[test]
fn plugins_read_granted_files_and_stream_large_json() {
    let source = voice(
        "local n, sum = 0, 0\n\
         for item in janky.fs.json_items('dict/items.json', '') do\n\
           for i = 1, 600 do end\n\
           n = n + 1\n\
           sum = sum + item\n\
         end\n\
         local names = table.concat(janky.fs.list('dict'), ',')\n\
         local text = janky.fs.read('dict/a.txt')\n\
         janky.http.download { url = 'https://a.example/x', file = 'x.bin' }\n\
         return { data = n .. ' ' .. sum .. ' ' .. names .. ' ' .. text .. ' ' .. janky.fs.read('cache/x.bin'), ext = 'ogg' }",
    );
    let (_work, root) = project_with(Some(&source), "\n[plugins.voice.filesystem]\nread = { dict = \"dict\" }\n");
    std::fs::create_dir(root.join("dict")).unwrap();
    std::fs::write(root.join("dict/a.txt"), "🧪⚗").unwrap();
    let items: Vec<String> = (0..20_000).map(|n| n.to_string()).collect();
    std::fs::write(root.join("dict/items.json"), format!("[{}]", items.join(","))).unwrap();
    let checked = check(&root);
    let http = Arc::new(FakeHttp::new(vec![ok(200, "cached", &[]), ok(200, "cached", &[])]));
    let results = run(&checked, &root, "voice", 1, http);
    let Outcome::Stored { file, .. } = &results[0].1 else { panic!("{results:?}") };
    let data = std::fs::read_to_string(root.join(".janky/cache/media").join(file)).unwrap();
    assert_eq!(data, "20000 199990000 a.txt,items.json 🧪⚗ cached");
}

fn prepared(prepare: &str) -> String {
    format!(
        "local function prepare(vars)\n{prepare}\nend\n\
         local function generate(job)\n\
           return {{ data = janky.cache.get('index')[job.input] .. ' ' .. janky.cache.get('runs'), ext = 'ogg' }}\n\
         end\n\
         return {{ id = 'voice', api = 1, generates = 'media', variables = {{ speed = {{ type = 'int' }}, voice = {{ type = 'string' }} }}, \
         generate = generate, prepare = prepare }}\n"
    )
}

#[test]
fn prepare_runs_once_before_the_jobs_of_its_instance() {
    let source = prepared(
        "janky.cache.put('runs', (janky.cache.get('runs') or 0) + 1)\n\
         janky.cache.put('index', { ['☿'] = 'mercury', ['🜚'] = 'gold' })",
    );
    let (_work, root) = project(Some(&source));
    let checked = check(&root);
    let jobs: Vec<_> = checked.rendered.jobs.iter().filter(|job| job.instance == "voice").cloned().collect();
    assert_eq!(jobs.len(), 2);
    let limits = HashMap::from([("voice".to_string(), 2)]);
    let mut started = Vec::new();
    let http = Arc::new(FakeHttp::default());
    let results = runner::run(
        &checked.plugins,
        &root,
        jobs,
        &limits,
        http,
        |instance| started.push(instance.to_string()),
        |_, _| true,
    );
    assert_eq!(started, ["voice"]);
    let mut data: Vec<String> = results
        .iter()
        .map(|(_, outcome)| {
            let Outcome::Stored { file, .. } = outcome else { panic!("{outcome:?}") };
            std::fs::read_to_string(root.join(".janky/cache/media").join(file)).unwrap()
        })
        .collect();
    data.sort();
    assert_eq!(data, ["gold 1", "mercury 1"]);
}

#[test]
fn a_failed_prepare_fails_its_jobs_and_keeps_no_cache_writes() {
    let source = prepared("janky.cache.put('partial', true)\nerror('broken index')");
    let (_work, root) = project(Some(&source));
    let checked = check(&root);
    let http = Arc::new(FakeHttp::default());
    let results = run(&checked, &root, "voice", 2, http.clone());
    assert_eq!(results.len(), 2);
    for (_, outcome) in &results {
        let Outcome::Failed(failure) = outcome else { panic!("{outcome:?}") };
        assert!(failure.message.contains("broken index"), "{}", failure.message);
    }
    let cache = janky_cli::plugin_cache::PluginCache::open(&root, "voice").unwrap();
    assert_eq!(cache.get("partial").unwrap(), None);
    assert!(http.requests.lock().unwrap().is_empty());
}

#[test]
fn a_generator_uses_janky_http_and_can_refuse() {
    let source = voice(
        "local r = janky.http.request { method = 'post', url = 'https://api.example.com/tts', \
         headers = { ['X-Key'] = 'k' }, body = job.input }\n\
         if r.status ~= 200 then return nil, 'HTTP ' .. r.status end\n\
         return { data = r.body, ext = 'ogg', cost = tonumber(r.headers['x-cost']) }",
    );
    let (_work, root) = project(Some(&source));
    let checked = check(&root);
    let http = Arc::new(FakeHttp::new(vec![ok(200, "audio", &[("X-Cost", "3")]), ok(500, "", &[])]));
    let results = run(&checked, &root, "voice", 1, http.clone());
    assert!(matches!(&results[0].1, Outcome::Stored { cost: Some(cost), .. } if *cost == 3.0), "{results:?}");
    assert!(matches!(&results[1].1, Outcome::Refused(message) if message == "HTTP 500"), "{results:?}");
    let requests = http.requests.lock().unwrap();
    assert_eq!(requests[0].0, "voice");
    assert_eq!(requests[0].1.method, "POST");
    assert_eq!(requests[0].1.url, "https://api.example.com/tts");
    assert_eq!(requests[0].1.headers, [("X-Key".to_string(), HeaderText::Text("k".into()))]);
    assert_eq!(requests[0].1.body, "☿".as_bytes());
}

#[test]
fn limits_apply_to_each_resume() {
    let endless = voice("while true do end");
    let (_work, root) = project(Some(&endless));
    let checked = check(&root);
    let results = run(&checked, &root, "voice", 1, Arc::new(FakeHttp::default()));
    let Outcome::Failed(failure) = &results[0].1 else { panic!("{results:?}") };
    assert!(failure.message.contains("more than 10000000 instructions"), "{}", failure.message);

    let paused = voice(
        "for i = 1, 3 do janky.wait(1) local n = 0 for j = 1, 3500000 do n = n + 1 end end\n\
         return { data = 'x', ext = 'ogg' }",
    );
    let (_work, root) = project(Some(&paused));
    let checked = check(&root);
    let results = run(&checked, &root, "voice", 1, Arc::new(FakeHttp::default()));
    assert!(results.iter().all(|(_, outcome)| matches!(outcome, Outcome::Stored { .. })), "{results:?}");

    let (_work, root) = project(Some(&paused.replace("janky.wait(1) ", "")));
    let checked = check(&root);
    let results = run(&checked, &root, "voice", 1, Arc::new(FakeHttp::default()));
    assert!(matches!(&results[0].1, Outcome::Failed(_)), "{results:?}");
}

#[test]
fn a_job_may_send_at_most_five_requests() {
    let source = voice("for i = 1, 6 do janky.http.request { url = 'https://api.example.com/' } end");
    let (_work, root) = project(Some(&source));
    let checked = check(&root);
    let responses = (0..12).map(|_| ok(200, "", &[])).collect();
    let http = Arc::new(FakeHttp::new(responses));
    let results = run(&checked, &root, "voice", 1, http.clone());
    let Outcome::Failed(failure) = &results[0].1 else { panic!("{results:?}") };
    assert!(failure.message.contains("at most 5 requests"), "{}", failure.message);
    assert_eq!(http.requests.lock().unwrap().len(), 10);
}

#[test]
fn jobs_wait_at_the_same_time_up_to_the_limit() {
    let source = voice("janky.wait(300) return { data = job.input, ext = 'ogg' }");
    for (limit, fast) in [(2, true), (1, false)] {
        let (_work, root) = project(Some(&source));
        let checked = check(&root);
        let start = Instant::now();
        let results = run(&checked, &root, "voice", limit, Arc::new(FakeHttp::default()));
        assert!(results.iter().all(|(_, outcome)| matches!(outcome, Outcome::Stored { .. })), "{results:?}");
        let elapsed = start.elapsed();
        assert_eq!(elapsed < Duration::from_millis(550), fast, "{elapsed:?} with limit {limit}");
    }
}

#[test]
fn host_functions_exist_only_inside_generate() {
    let source = "local function plan(job) return { ext = 'ogg', key = tostring(janky.http) .. tostring(janky.wait) .. tostring(janky.cache) .. tostring(janky.fs) } end\n\
                  return { id = 'voice', api = 1, generates = 'media', variables = { speed = { type = 'int' }, voice = { type = 'string' } }, \
                  generate = function() return { data = tostring(janky.http ~= nil and janky.cache ~= nil), ext = 'ogg' } end, plan = plan }\n";
    let (_work, root) = project(Some(source));
    let checked = check(&root);
    assert!(checked.rendered.jobs.iter().filter(|job| job.instance == "voice").all(|job| job.key == "nilnilnilnil"));
    let results = run(&checked, &root, "voice", 1, Arc::new(FakeHttp::default()));
    let Outcome::Stored { file, .. } = &results[0].1 else { panic!("{results:?}") };
    assert_eq!(std::fs::read(root.join(".janky/cache/media").join(file)).unwrap(), b"true");
    let again = check(&root);
    assert!(again.rendered.jobs.iter().filter(|job| job.instance == "voice").all(|job| job.key == "nilnilnilnil"));
}

#[test]
fn a_stop_starts_no_more_jobs() {
    let (_work, root) = project(None);
    let checked = check(&root);
    let jobs: Vec<_> = checked.rendered.jobs.clone();
    let limits = HashMap::from([("voice".to_string(), 1), ("plain".to_string(), 1)]);
    let mut seen = 0;
    let results = runner::run(
        &checked.plugins,
        &root,
        jobs,
        &limits,
        Arc::new(FakeHttp::default()),
        |_| {},
        |_, _| {
            seen += 1;
            false
        },
    );
    assert_eq!(results.len(), 4);
    assert_eq!(seen, 2);
    assert_eq!(results.iter().filter(|(_, outcome)| matches!(outcome, Outcome::Skipped)).count(), 2);
}

#[test]
fn wrong_results_are_errors() {
    for (body, message) in [
        ("return 'x'", "must return a table"),
        ("return { data = 'x', ext = 'mp3' }", "but `plan` gave `ogg`"),
        ("return { data = 'x', ext = '../x' }", "must return `ext`"),
        ("return { data = 'x', ext = 'json' }", "not `json`"),
        ("return { value = {} }", "returned `value` for a media job"),
        ("return { ext = 'ogg' }", "must return `data`"),
        (
            "janky.http.request { url = 'https://a.example', colour = 1 }",
            "only `method`, `url`, `headers` and `body`",
        ),
        ("janky.http.request { url = 'https://a.example', method = 'TRACE' }", "`method` must be one of"),
        ("janky.http.request { url = 'https://a.example', headers = { a = 'x\\r\\nb: y' } }", "line break"),
        (
            "janky.http.request { url = 'https://a.example', headers = { Host = 'internal' } }",
            "cannot set the header `Host`",
        ),
        (
            "janky.http.request { url = 'https://a.example', headers = { ['Accept-Encoding'] = 'gzip' } }",
            "cannot set the header",
        ),
        ("janky.cache.put('', 1)", "a plugin cache key has 1 to"),
        ("janky.cache.put('k', function() end)", "function"),
        ("janky.http.download { url = 'https://a.example', file = '../x' }", "is not a valid file name"),
        ("janky.http.download { url = 'https://a.example', file = '.hidden' }", "is not a valid file name"),
        ("janky.http.download { url = 'https://a.example' }", "a download needs `file`"),
        ("janky.http.download { url = 'https://a.example', file = 'a', colour = 1 }", "a download has only"),
    ] {
        let (_work, root) = project(Some(&voice(body)));
        let checked = check(&root);
        let results = run(&checked, &root, "voice", 1, Arc::new(FakeHttp::new(vec![ok(200, "", &[])])));
        let Outcome::Failed(failure) = &results[0].1 else {
            panic!("{body}: {results:?}")
        };
        assert!(failure.message.contains(message), "{body}: {}", failure.message);
    }
}

fn secret_voice(generate: &str) -> String {
    format!(
        "local function plan(job) return {{ ext = 'ogg' }} end\n\
         local function generate(job, vars)\n{generate}\nend\n\
         return {{ id = 'voice', api = 1, generates = 'media', secrets = {{ 'api_key' }}, \
         variables = {{ speed = {{ type = 'int' }}, voice = {{ type = 'string' }} }}, generate = generate, plan = plan }}\n"
    )
}

const SECRET_DECK: &str = "\n[plugins.voice.secrets]\napi_key = { file = \"key.txt\" }\n";

#[test]
fn secret_handles_can_only_be_header_values() {
    let source = secret_voice(
        "local key = janky.secret('api_key')\n\
         local checks = { tostring(key) == '<secret:api_key>', pcall(function() return 'x' .. key end), \
         pcall(janky.json.encode, { key }), pcall(janky.secret, 'other'), (pcall(janky.secret, 'api_key', 'a\\n')) }\n\
         janky.http.request { url = 'https://api.example.com/', headers = { ['xi-api-key'] = key, \
         authorization = janky.secret('api_key', 'Bearer ') } }\n\
         local text = {} for i, v in ipairs(checks) do text[i] = tostring(v) end\n\
         return { data = table.concat(text, ','), ext = 'ogg' }",
    );
    let (_work, root) = project_with(Some(&source), SECRET_DECK);
    let checked = check(&root);
    let http = Arc::new(FakeHttp::new(vec![ok(200, "", &[]), ok(200, "", &[])]));
    let results = run(&checked, &root, "voice", 1, http.clone());
    let Outcome::Stored { file, .. } = &results[0].1 else { panic!("{results:?}") };
    let data = std::fs::read_to_string(root.join(".janky/cache/media").join(file)).unwrap();
    assert_eq!(data, "true,false,false,false,false");
    let requests = http.requests.lock().unwrap();
    assert_eq!(
        requests[0].1.headers,
        [
            (
                "authorization".to_string(),
                HeaderText::Secret {
                    name: "api_key".into(),
                    prefix: "Bearer ".into()
                }
            ),
            (
                "xi-api-key".to_string(),
                HeaderText::Secret {
                    name: "api_key".into(),
                    prefix: String::new()
                }
            ),
        ]
    );

    let (_work, root) =
        project_with(Some(&secret_voice("return { data = janky.secret('api_key'), ext = 'ogg' }")), SECRET_DECK);
    let checked = check(&root);
    let results = run(&checked, &root, "voice", 1, Arc::new(FakeHttp::default()));
    assert!(
        matches!(&results[0].1, Outcome::Failed(failure) if failure.message.contains("`data` as a string")),
        "{results:?}"
    );
}

#[test]
fn a_refused_credential_names_the_secret() {
    let source = secret_voice(
        "local r = janky.http.request { url = 'https://api.example.com/', headers = { ['xi-api-key'] = janky.secret('api_key') } }\n\
         if r.status ~= 200 then return nil, 'HTTP ' .. r.status end\n\
         return { data = r.body, ext = 'ogg' }",
    );
    let (_work, root) = project_with(Some(&source), SECRET_DECK);
    let checked = check(&root);
    let http = Arc::new(FakeHttp::new(vec![ok(401, "quota_exceeded", &[]), ok(200, "a", &[])]));
    let results = run(&checked, &root, "voice", 1, http);
    assert!(
        matches!(&results[0].1, Outcome::Unauthorised { message, secrets } if message == "HTTP 401" && secrets == &["api_key"]),
        "{results:?}"
    );
    assert!(matches!(&results[1].1, Outcome::Stored { .. }), "{results:?}");
}

#[test]
fn missing_secrets_are_reported_before_anything_runs() {
    let (_work, root) = project_with(Some(&secret_voice("return nil, 'x'")), SECRET_DECK);
    let checked = check(&root);
    let mut diagnostics = Diagnostics::default();
    janky_cli::secrets::resolve(&checked.project, &checked.plugins, &mut diagnostics);
    let text = diagnostics.to_json_lines();
    assert!(text.contains("secret::missing") && text.contains("the secret `api_key` of `voice`"), "{text}");
    std::fs::write(root.join("key.txt"), "file-key-0123456789\n").unwrap();
    let mut diagnostics = Diagnostics::default();
    let secrets = janky_cli::secrets::resolve(&checked.project, &checked.plugins, &mut diagnostics);
    assert!(diagnostics.is_empty());
    assert_eq!(secrets.get("voice", "api_key"), Some("file-key-0123456789"));
    assert_eq!(janky_cli::diagnostic::visible("echo file-key-0123456789"), "echo <secret:api_key>");
}
