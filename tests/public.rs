//! The public, multi-user server: sessions, limits and what it refuses.

use base64::Engine as _;
use shacl_report::input::{LoadLimits, MemoryFile};
use shacl_report::report::load_report_from_memory_limited;
use shacl_report::web::public::PublicConfig;
use shacl_report::web::{Preload, build_state, start};
use std::path::PathBuf;
use std::time::Duration;

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name),
    )
    .unwrap()
}

fn cfg() -> PublicConfig {
    PublicConfig {
        max_sessions: 1,
        max_sessions_per_ip: 1,
        session_idle: Duration::from_secs(60),
        max_jobs: 1,
        queue_wait: Duration::from_secs(5),
        job_time: Duration::from_secs(30),
        max_upload_bytes: 1024 * 1024,
        max_triples: 10_000,
        allow_endpoints: vec!["https://example.org/sparql".into()],
        trust_proxy: false,
    }
}

fn upload_body() -> String {
    let enc = |name: &str| {
        serde_json::json!({
            "name": name,
            "content": base64::engine::general_purpose::STANDARD.encode(fixture(name)),
        })
    };
    serde_json::json!({ "shapes": [enc("shapes.ttl")], "data": [enc("data.ttl")] }).to_string()
}

#[test]
fn caps_triples_and_decompressed_size() {
    let file = |name: &str| MemoryFile {
        name: name.to_string(),
        content: fixture(name),
    };
    let few = LoadLimits {
        max_triples: Some(5),
        ..LoadLimits::default()
    };
    let err =
        load_report_from_memory_limited(None, &[file("shapes.ttl")], &[file("data.ttl")], &few)
            .err()
            .unwrap();
    assert!(
        format!("{err:#}").contains("more than 5 triples"),
        "{err:#}"
    );

    // 10 MB of zeros gzip to a few kB; the cap applies to the unpacked size.
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
    std::io::Write::write_all(&mut gz, &vec![b' '; 10 * 1024 * 1024]).unwrap();
    let bomb = MemoryFile {
        name: "bomb.ttl.gz".into(),
        content: gz.finish().unwrap(),
    };
    let small = LoadLimits {
        max_bytes: Some(1024 * 1024),
        ..LoadLimits::default()
    };
    let err = load_report_from_memory_limited(None, &[bomb], &[], &small)
        .err()
        .unwrap();
    assert!(
        format!("{err:#}").contains("larger than this server accepts"),
        "{err:#}"
    );
}

#[test]
fn gives_each_visitor_their_own_session() {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let addr = rt.block_on(async {
        let state = build_state(Preload::None, None, Some(cfg())).await.unwrap();
        let server = start("127.0.0.1:0", state).await.unwrap();
        let addr = server.addr;
        tokio::spawn(server.join());
        addr
    });
    let base = format!("http://{addr}");
    // The server runs on `rt`'s thread only while we block on it, so do the
    // blocking HTTP calls from another thread.
    let client = std::thread::spawn(move || {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .build()
            .into();
        let meta = |cookie: Option<&str>| {
            let mut r = agent.get(format!("{base}/api/meta"));
            if let Some(c) = cookie {
                r = r.header("Cookie", c);
            }
            r.call().unwrap().into_body().read_to_string().unwrap()
        };
        assert!(meta(None).contains("\"loaded\":false"));

        let resp = agent
            .post(format!("{base}/api/upload"))
            .header("Content-Type", "application/json")
            .send(upload_body())
            .unwrap();
        assert_eq!(resp.status(), 200);
        let cookie = resp.headers().get("set-cookie").unwrap().to_str().unwrap();
        let cookie = cookie.split(';').next().unwrap().to_string();

        // Only the uploader sees the report.
        assert!(meta(Some(&cookie)).contains("\"loaded\":true"));
        assert!(meta(None).contains("\"loaded\":false"));

        // The one slot is taken: a second visitor is asked to come back later.
        let other = agent
            .post(format!("{base}/api/upload"))
            .header("Content-Type", "application/json")
            .send(upload_body())
            .unwrap();
        assert_eq!(other.status(), 503);
        assert!(other.headers().get("retry-after").is_some());

        // No server-side paths, and the relay only reaches allowed endpoints.
        let load = agent
            .post(format!("{base}/api/load"))
            .header("Content-Type", "application/json")
            .send(r#"{"shapes":["/etc/passwd"]}"#)
            .unwrap();
        assert_eq!(load.status(), 403);
        let relay = agent
            .get(format!(
                "{base}/api/sparql?endpoint=http%3A%2F%2F169.254.169.254%2F&query=ASK%7B%7D"
            ))
            .call()
            .unwrap();
        assert_eq!(relay.status(), 403);

        // Ending the session frees the slot for the next visitor.
        let end = agent
            .delete(format!("{base}/api/session"))
            .header("Cookie", &cookie)
            .call()
            .unwrap();
        assert_eq!(end.status(), 204);
        let next = agent
            .post(format!("{base}/api/upload"))
            .header("Content-Type", "application/json")
            .send(upload_body())
            .unwrap();
        assert_eq!(next.status(), 200);
    });
    rt.block_on(async {
        while !client.is_finished() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    });
    client.join().unwrap();
}
