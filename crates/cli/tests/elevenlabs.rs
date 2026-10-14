use std::collections::HashMap;
use std::sync::Arc;

use janky_cli::diagnostic::Diagnostics;
use janky_cli::runner::{self, FakeHttp, HeaderText, Outcome, Response};

mod common;

fn response(status: u16, headers: &[(&str, &str)], body: &[u8]) -> Result<Response, String> {
    let headers = headers.iter().map(|(name, value)| (name.to_string(), value.to_string())).collect();
    Ok(Response { status, headers, body: body.to_vec() })
}

#[test]
fn the_elevenlabs_plugin_builds_requests_and_reads_responses() {
    let (_work, dir) = common::example();
    let mut diagnostics = Diagnostics::default();
    let checked = janky_cli::cli::check(&dir.join("deck.toml"), Default::default(), &mut diagnostics, &mut Vec::new());
    assert_eq!(diagnostics.to_json_lines(), "");
    let checked = checked.unwrap();
    let jobs: Vec<_> = checked.rendered.jobs.iter().filter(|job| job.instance == "voice").cloned().collect();
    assert_eq!(jobs.len(), 5);
    assert!(jobs.iter().all(|job| job.ext.as_deref() == Some("ogg") && job.unit.as_deref() == Some("characters")));
    assert_eq!(jobs[0].units, Some(jobs[0].input_text().chars().count() as u64));

    let quota = br#"{"detail":{"status":"quota_exceeded","message":"This request exceeds your quota of 10000."}}"#;
    let busy = br#"{"detail":{"status":"too_many_concurrent_requests","message":"Too many concurrent requests."}}"#;
    let http = Arc::new(FakeHttp::new(vec![
        response(
            200,
            &[
                ("Content-Type", "audio/ogg"),
                ("character-cost", "3"),
                ("request-id", "r1"),
            ],
            b"OggS audio",
        ),
        response(401, &[("Content-Type", "application/json")], quota),
        response(429, &[("Content-Type", "application/json")], busy),
        response(500, &[("Content-Type", "text/plain")], b"Internal Server Error"),
        response(
            422,
            &[("Content-Type", "application/json")],
            br#"{"detail":[{"loc":["body","model_id"],"msg":"Model not found","type":"value_error"}]}"#,
        ),
    ]));
    let limits = HashMap::from([("voice".to_string(), 1)]);
    let results = runner::run(&checked.plugins, &dir, jobs.clone(), &limits, http.clone(), |_| {}, |_, _| true);

    assert!(matches!(&results[0].1, Outcome::Stored { cost: Some(cost), .. } if *cost == 3.0), "{results:?}");
    assert!(
        matches!(&results[1].1, Outcome::Unauthorised { message, secrets }
            if message == "ElevenLabs HTTP 401: This request exceeds your quota of 10000." && secrets == &["api_key"]),
        "{results:?}"
    );
    assert!(
        matches!(&results[2].1, Outcome::Refused(message) if message == "ElevenLabs HTTP 429: Too many concurrent requests.")
    );
    assert!(
        matches!(&results[3].1, Outcome::Refused(message) if message == "ElevenLabs HTTP 500: Internal Server Error")
    );
    assert!(
        matches!(&results[4].1, Outcome::Refused(message) if message == "ElevenLabs HTTP 422: Model not found"),
        "{results:?}"
    );
    let Outcome::Stored { file, .. } = &results[0].1 else { unreachable!() };
    assert_eq!(std::fs::read(dir.join(".janky/cache/media").join(file)).unwrap(), b"OggS audio");

    let requests = http.requests.lock().unwrap();
    let (instance, request) = &requests[0];
    assert_eq!(instance, "voice");
    assert_eq!(request.method, "POST");
    assert_eq!(
        request.url,
        "https://api.elevenlabs.io/v1/text-to-speech/JBFqnCBsd6RMkjVDRZzb?output_format=opus_48000_64"
    );
    assert_eq!(
        request.headers,
        [
            ("accept".to_string(), HeaderText::Text("audio/*".into())),
            ("content-type".to_string(), HeaderText::Text("application/json".into())),
            (
                "xi-api-key".to_string(),
                HeaderText::Secret {
                    name: "api_key".into(),
                    prefix: String::new()
                }
            ),
        ]
    );
    let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
    assert_eq!(
        body,
        serde_json::json!({
            "text": jobs[0].input_text(),
            "model_id": "eleven_v4",
            "language_code": "en",
            "voice_settings": { "stability": 0.35, "similarity_boost": 0.75 },
        })
    );
}
