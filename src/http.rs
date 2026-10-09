//! Minimal HTTP client used to fetch RDF from URLs and to relay SPARQL
//! requests from the viewer to a user-chosen endpoint (avoids CORS).

use crate::input::MemoryFile;
use anyhow::{Context, Result, bail};
use std::time::Duration;
use ureq::Agent;

/// Largest response body accepted from a remote server.
const MAX_BODY_BYTES: u64 = 1024 * 1024 * 1024;

const RDF_ACCEPT: &str = "text/turtle, application/n-triples;q=0.9, application/trig;q=0.8, \
                          application/n-quads;q=0.8, application/rdf+xml;q=0.7, */*;q=0.1";

/// True when `s` should be fetched over HTTP instead of read from disk.
pub fn is_url(s: &str) -> bool {
    let l = s.to_ascii_lowercase();
    l.starts_with("http://") || l.starts_with("https://")
}

fn agent() -> Agent {
    Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(300)))
        .build()
        .into()
}

/// Map a response media type to a file extension the parser understands.
fn extension_for(content_type: &str) -> Option<&'static str> {
    let mt = content_type.split(';').next()?.trim().to_ascii_lowercase();
    Some(match mt.as_str() {
        "text/turtle" | "application/x-turtle" => "ttl",
        "application/n-triples" => "nt",
        "application/trig" => "trig",
        "application/n-quads" => "nq",
        "application/rdf+xml" | "application/xml" | "text/xml" => "rdf",
        _ => return None,
    })
}

/// Download an RDF document. The returned name carries an extension derived
/// from the response's media type (falling back to the URL's own extension).
pub fn fetch_rdf(url: &str) -> Result<MemoryFile> {
    let resp = agent()
        .get(url)
        .header("Accept", RDF_ACCEPT)
        .call()
        .with_context(|| format!("failed to fetch {url}"))?;
    let status = resp.status().as_u16();
    if !(200..300).contains(&status) {
        bail!("failed to fetch {url}: HTTP {status}");
    }
    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let content = resp
        .into_body()
        .into_with_config()
        .limit(MAX_BODY_BYTES)
        .read_to_vec()
        .with_context(|| format!("failed to read {url}"))?;
    let base = url
        .split(['?', '#'])
        .next()
        .unwrap_or(url)
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or("download")
        .to_string();
    let name = match extension_for(&content_type) {
        Some(ext) => format!("{base}.{ext}"),
        None => base,
    };
    Ok(MemoryFile { name, content })
}

/// Response relayed back to the browser.
pub struct Relayed {
    pub status: u16,
    pub content_type: String,
    pub body: Vec<u8>,
}

/// Forward a SPARQL protocol request to `endpoint`.
pub fn relay_sparql(
    endpoint: &str,
    query_string: Option<&str>,
    body: Option<(Vec<u8>, String)>,
    accept: Option<&str>,
    authorization: Option<&str>,
) -> Result<Relayed> {
    if !is_url(endpoint) {
        bail!("endpoint must be an http(s) URL");
    }
    let target = match query_string {
        Some(q) if !q.is_empty() => {
            let sep = if endpoint.contains('?') { '&' } else { '?' };
            format!("{endpoint}{sep}{q}")
        }
        _ => endpoint.to_string(),
    };
    let agent = agent();
    let accept = accept.unwrap_or("application/sparql-results+json, */*;q=0.5");
    let resp = match body {
        Some((bytes, content_type)) => {
            let mut req = agent
                .post(&target)
                .header("Accept", accept)
                .header("Content-Type", content_type);
            if let Some(a) = authorization {
                req = req.header("Authorization", a);
            }
            req.send(&bytes[..])
        }
        None => {
            let mut req = agent.get(&target).header("Accept", accept);
            if let Some(a) = authorization {
                req = req.header("Authorization", a);
            }
            req.call()
        }
    }
    .with_context(|| format!("failed to reach {endpoint}"))?;
    let status = resp.status().as_u16();
    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("text/plain")
        .to_string();
    let body = resp
        .into_body()
        .into_with_config()
        .limit(MAX_BODY_BYTES)
        .read_to_vec()
        .context("failed to read the endpoint's response")?;
    Ok(Relayed {
        status,
        content_type,
        body,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_media_types() {
        assert_eq!(extension_for("text/turtle; charset=utf-8"), Some("ttl"));
        assert_eq!(extension_for("application/rdf+xml"), Some("rdf"));
        assert_eq!(extension_for("text/html"), None);
        assert!(is_url("HTTPS://example.org/x.ttl"));
        assert!(!is_url("/tmp/x.ttl"));
    }
}
