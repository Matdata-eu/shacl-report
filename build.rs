//! Fetch the popular prefixes from prefix.cc at build time, as MatGUI does.
//!
//! The list is written to `OUT_DIR/prefixes.json` and embedded in the binary.
//! When prefix.cc cannot be reached (offline builds, docs.rs, a bad
//! certificate on both HTTPS and HTTP), the committed `assets/prefixes.json`
//! is used instead, without a warning, and the build carries on. Cargo does not allow a build
//! script to change files in the source tree, so the committed copy is
//! refreshed by hand (see the README) rather than here.

use std::path::PathBuf;
use std::time::Duration;

const FALLBACK: &str = "assets/prefixes.json";
const URLS: [&str; 2] = [
    "https://prefix.cc/popular/all.file.json",
    "http://prefix.cc/popular/all.file.json",
];

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed={FALLBACK}");
    for var in ["SHACL_REPORT_OFFLINE", "CARGO_NET_OFFLINE", "DOCS_RS"] {
        println!("cargo:rerun-if-env-changed={var}");
    }
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("prefixes.json");
    let fetched = if offline() { None } else { fetch() };
    let data = match fetched {
        Some(data) => data,
        None => std::fs::read_to_string(FALLBACK).expect("assets/prefixes.json is missing"),
    };
    std::fs::write(out, data).unwrap();
}

fn offline() -> bool {
    let set = |v: &str| std::env::var(v).is_ok_and(|x| !x.is_empty() && x != "0" && x != "false");
    set("SHACL_REPORT_OFFLINE") || set("CARGO_NET_OFFLINE") || set("DOCS_RS")
}

/// The prefix list from prefix.cc, or `None` on any failure.
fn fetch() -> Option<String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(10)))
        .build()
        .into();
    for url in URLS {
        let Ok(resp) = agent.get(url).call() else {
            continue;
        };
        let Ok(body) = resp.into_body().read_to_string() else {
            continue;
        };
        // Only accept a JSON object of prefix to namespace strings, and only a
        // plausible one: an error page or an empty answer keeps the fallback.
        match serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&body) {
            Ok(map) if map.len() > 100 && map.values().all(|v| v.is_string()) => return Some(body),
            _ => continue,
        }
    }
    // Fail silently: the committed list is a fine substitute.
    None
}
