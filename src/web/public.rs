//! Multi-user mode for running the viewer as a public website.
//!
//! The local viewer keeps one report for whoever has the page open. A public
//! server instead gives every visitor a session of their own and bounds what
//! visitors together can cost:
//!
//! - **Memory**: a session only exists once a visitor has loaded files, and at
//!   most `max_sessions` exist at once (`max_sessions_per_ip` per address).
//!   Each holds at most one upload of `max_upload_mb` / `max_triples`.
//!   Sessions idle for `session_idle` are dropped, freeing their slot.
//! - **CPU**: parsing and validation run in at most `max_jobs` parallel jobs.
//!   Further uploads queue for up to `queue_wait`, then get a "busy" answer.
//!   A session runs one job at a time, and a validation stops after `job_time`.
//! - **Reach**: the server reads no files and fetches no URLs for visitors,
//!   and the SPARQL relay only forwards to allow-listed endpoints.
//!
//! Visitors without a session (crawlers, health checks, someone just looking)
//! cost nothing beyond serving static pages.

use crate::cli::PublicArgs;
use crate::input::LoadLimits;
use crate::report::Report;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

const COOKIE: &str = "shacl_report_session";

/// Settings of a public server, from [`PublicArgs`].
#[derive(Debug, Clone)]
pub struct PublicConfig {
    pub max_sessions: usize,
    pub max_sessions_per_ip: usize,
    pub session_idle: Duration,
    pub max_jobs: usize,
    pub queue_wait: Duration,
    pub job_time: Duration,
    pub max_upload_bytes: u64,
    pub max_triples: usize,
    pub allow_endpoints: Vec<String>,
    pub trust_proxy: bool,
}

impl PublicConfig {
    pub fn from_args(a: &PublicArgs, endpoint: Option<&str>) -> Self {
        let mut allow: Vec<String> = a
            .allow_endpoint
            .iter()
            .map(|e| e.trim().to_string())
            .filter(|e| !e.is_empty())
            .collect();
        if let Some(e) = endpoint {
            allow.push(e.trim().to_string());
        }
        allow.dedup();
        Self {
            max_sessions: a.max_sessions.max(1),
            max_sessions_per_ip: a.max_sessions_per_ip.max(1),
            session_idle: Duration::from_secs(a.session_idle_minutes.max(1) * 60),
            max_jobs: a.max_jobs.max(1),
            queue_wait: Duration::from_secs(a.queue_seconds),
            job_time: Duration::from_secs(a.job_seconds.max(1)),
            max_upload_bytes: a.max_upload_mb.max(1) * 1024 * 1024,
            max_triples: a.max_triples.max(1),
            allow_endpoints: allow,
            trust_proxy: a.trust_proxy,
        }
    }

    /// Largest request body for an upload: base64 grows files by a third.
    pub fn max_body_bytes(&self) -> usize {
        usize::try_from(self.max_upload_bytes / 3 * 4 + 64 * 1024).unwrap_or(usize::MAX)
    }

    pub fn relay_allowed(&self, endpoint: &str) -> bool {
        self.allow_endpoints.iter().any(|e| e == endpoint.trim())
    }
}

/// What the viewer needs to know about the server's limits.
#[derive(Serialize)]
pub struct PublicInfo {
    max_upload_mb: u64,
    max_triples: usize,
    session_idle_minutes: u64,
    allow_endpoints: Vec<String>,
}

struct Session {
    report: Option<Arc<Report>>,
    ip: IpAddr,
    last_seen: Instant,
    busy: bool,
}

/// Sessions and job slots of a public server.
pub struct Public {
    pub cfg: PublicConfig,
    sessions: Mutex<HashMap<String, Session>>,
    jobs: Arc<Semaphore>,
}

/// Why a visitor cannot start a job right now.
pub enum Refusal {
    Full,
    PerIp,
    Busy,
    Running,
}

impl IntoResponse for Refusal {
    fn into_response(self) -> Response {
        let (status, msg) = match self {
            Refusal::Full => (
                StatusCode::SERVICE_UNAVAILABLE,
                "The server is at its limit of active users. Please try again in a few minutes.",
            ),
            Refusal::PerIp => (
                StatusCode::TOO_MANY_REQUESTS,
                "Your network already has the most sessions this server allows. Close another tab or wait until one expires.",
            ),
            Refusal::Busy => (
                StatusCode::SERVICE_UNAVAILABLE,
                "The server is busy validating files for other users. Please try again in a minute.",
            ),
            Refusal::Running => (
                StatusCode::CONFLICT,
                "Your previous files are still being processed. Wait for them to finish first.",
            ),
        };
        let mut resp = (status, msg).into_response();
        if status != StatusCode::CONFLICT {
            resp.headers_mut()
                .insert(header::RETRY_AFTER, HeaderValue::from_static("60"));
        }
        resp
    }
}

/// A visitor's claim on a session and a job slot while their upload runs.
/// Dropping it without [`Job::finish`] leaves the session as it was.
pub struct Job {
    public: Arc<Public>,
    pub id: String,
    /// Set when the session was created for this job; send it as a cookie.
    pub new_cookie: Option<HeaderValue>,
    _permit: OwnedSemaphorePermit,
}

impl Job {
    pub fn limits(&self) -> LoadLimits {
        let cfg = &self.public.cfg;
        LoadLimits {
            max_bytes: Some(cfg.max_upload_bytes),
            max_triples: Some(cfg.max_triples),
            deadline: Some(Instant::now() + cfg.job_time),
        }
    }

    /// Store the job's result in the session (on success) and release it.
    pub fn finish(self, report: Option<Arc<Report>>) {
        let mut map = self.public.sessions.lock().unwrap();
        let fresh = self.new_cookie.is_some();
        if let Some(s) = map.get_mut(&self.id) {
            s.busy = false;
            s.last_seen = Instant::now();
            if report.is_some() {
                s.report = report;
            } else if fresh {
                // A failed first upload should not hold a slot.
                map.remove(&self.id);
            }
        }
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        // Covers early returns and panics: never leave a session marked busy.
        if let Ok(mut map) = self.public.sessions.lock()
            && let Some(s) = map.get_mut(&self.id)
        {
            s.busy = false;
        }
    }
}

impl Public {
    pub fn new(cfg: PublicConfig) -> Arc<Self> {
        Arc::new(Self {
            jobs: Arc::new(Semaphore::new(cfg.max_jobs)),
            sessions: Mutex::new(HashMap::new()),
            cfg,
        })
    }

    pub fn info(&self) -> PublicInfo {
        PublicInfo {
            max_upload_mb: self.cfg.max_upload_bytes / 1024 / 1024,
            max_triples: self.cfg.max_triples,
            session_idle_minutes: self.cfg.session_idle.as_secs() / 60,
            allow_endpoints: self.cfg.allow_endpoints.clone(),
        }
    }

    /// The visitor's IP: the socket peer, or behind a trusted proxy the last
    /// X-Forwarded-For entry (the one that proxy appended itself).
    pub fn client_ip(&self, headers: &HeaderMap, peer: Option<IpAddr>) -> IpAddr {
        if self.cfg.trust_proxy
            && let Some(ip) = headers
                .get_all("x-forwarded-for")
                .iter()
                .next_back()
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.rsplit(',').next())
                .and_then(|s| s.trim().parse::<IpAddr>().ok())
        {
            return ip;
        }
        peer.unwrap_or(IpAddr::from([0, 0, 0, 0]))
    }

    /// Drop sessions idle for longer than the configured time.
    pub fn sweep(&self) {
        let idle = self.cfg.session_idle;
        let now = Instant::now();
        self.sessions
            .lock()
            .unwrap()
            .retain(|_, s| s.busy || now.duration_since(s.last_seen) < idle);
    }

    /// The report of the visitor's session, refreshing its idle timer.
    /// `None` when they have no live session.
    pub fn touch(&self, headers: &HeaderMap) -> Option<Option<Arc<Report>>> {
        let id = session_cookie(headers)?;
        let mut map = self.sessions.lock().unwrap();
        let s = map.get_mut(id)?;
        if Instant::now().duration_since(s.last_seen) >= self.cfg.session_idle && !s.busy {
            map.remove(id);
            return None;
        }
        s.last_seen = Instant::now();
        Some(s.report.clone())
    }

    /// Claim the visitor's session (creating one if needed) and wait for a
    /// job slot. Refuses when the server or the visitor is at a limit.
    pub async fn begin(self: &Arc<Self>, headers: &HeaderMap, ip: IpAddr) -> Result<Job, Refusal> {
        self.sweep();
        let (id, new_cookie) = {
            let mut map = self.sessions.lock().unwrap();
            let existing = session_cookie(headers).filter(|id| map.contains_key(*id));
            match existing {
                Some(id) => {
                    let s = map.get_mut(id).unwrap();
                    if s.busy {
                        return Err(Refusal::Running);
                    }
                    s.busy = true;
                    s.last_seen = Instant::now();
                    (id.to_string(), None)
                }
                None => {
                    if map.len() >= self.cfg.max_sessions {
                        return Err(Refusal::Full);
                    }
                    if map.values().filter(|s| s.ip == ip).count() >= self.cfg.max_sessions_per_ip {
                        return Err(Refusal::PerIp);
                    }
                    let id = new_id();
                    map.insert(
                        id.clone(),
                        Session {
                            report: None,
                            ip,
                            last_seen: Instant::now(),
                            busy: true,
                        },
                    );
                    let cookie = format!("{COOKIE}={id}; Path=/; HttpOnly; SameSite=Lax");
                    (id, Some(HeaderValue::from_str(&cookie).unwrap()))
                }
            }
        };
        let permit =
            tokio::time::timeout(self.cfg.queue_wait, self.jobs.clone().acquire_owned()).await;
        let job = |permit| Job {
            public: self.clone(),
            id: id.clone(),
            new_cookie: new_cookie.clone(),
            _permit: permit,
        };
        match permit {
            Ok(Ok(p)) => Ok(job(p)),
            _ => {
                let mut map = self.sessions.lock().unwrap();
                if new_cookie.is_some() {
                    map.remove(&id);
                } else if let Some(s) = map.get_mut(&id) {
                    s.busy = false;
                }
                Err(Refusal::Busy)
            }
        }
    }

    /// End the visitor's session now, freeing its slot.
    pub fn end(&self, headers: &HeaderMap) {
        if let Some(id) = session_cookie(headers) {
            let mut map = self.sessions.lock().unwrap();
            if map.get(id).is_some_and(|s| !s.busy) {
                map.remove(id);
            }
        }
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.sessions.lock().unwrap().len()
    }
}

fn session_cookie(headers: &HeaderMap) -> Option<&str> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|kv| kv.trim().split_once('='))
        .find(|(k, _)| *k == COOKIE)
        .map(|(_, v)| v)
}

/// 128 random bits as hex: unguessable, so one visitor cannot open another's session.
fn new_id() -> String {
    let mut b = [0u8; 16];
    getrandom::fill(&mut b).expect("the OS random number generator failed");
    b.iter().map(|x| format!("{x:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> PublicConfig {
        PublicConfig {
            max_sessions: 2,
            max_sessions_per_ip: 1,
            session_idle: Duration::from_secs(60),
            max_jobs: 1,
            queue_wait: Duration::from_millis(50),
            job_time: Duration::from_secs(5),
            max_upload_bytes: 1024,
            max_triples: 10,
            allow_endpoints: vec!["https://example.org/sparql".into()],
            trust_proxy: false,
        }
    }

    fn cookie(job: &Job) -> HeaderMap {
        let mut h = HeaderMap::new();
        let v = job.new_cookie.as_ref().unwrap().to_str().unwrap();
        h.insert(
            header::COOKIE,
            v.split(';').next().unwrap().parse().unwrap(),
        );
        h
    }

    const A: IpAddr = IpAddr::V4(std::net::Ipv4Addr::new(10, 0, 0, 1));
    const B: IpAddr = IpAddr::V4(std::net::Ipv4Addr::new(10, 0, 0, 2));
    const C: IpAddr = IpAddr::V4(std::net::Ipv4Addr::new(10, 0, 0, 3));

    #[tokio::test]
    async fn limits_sessions_globally_and_per_ip() {
        let p = Public::new(cfg());
        let none = HeaderMap::new();
        let a = p.begin(&none, A).await.ok().unwrap();
        let ha = cookie(&a);
        a.finish(Some(Arc::new(Report::default())));
        assert!(matches!(p.begin(&none, A).await, Err(Refusal::PerIp)));
        let b = p.begin(&none, B).await.ok().unwrap();
        b.finish(Some(Arc::new(Report::default())));
        assert!(matches!(p.begin(&none, C).await, Err(Refusal::Full)));
        // An existing session can always load new files.
        p.begin(&ha, A).await.ok().unwrap().finish(None);
        assert!(p.touch(&ha).unwrap().is_some());
        p.end(&ha);
        assert_eq!(p.len(), 1);
        assert!(p.begin(&none, C).await.is_ok());
    }

    #[tokio::test]
    async fn queues_jobs_and_drops_failed_first_uploads() {
        let p = Public::new(cfg());
        let none = HeaderMap::new();
        let a = p.begin(&none, A).await.ok().unwrap();
        // The only job slot is taken: B waits, then is told the server is busy.
        assert!(matches!(p.begin(&none, B).await, Err(Refusal::Busy)));
        assert!(matches!(
            p.begin(&cookie(&a), A).await,
            Err(Refusal::Running)
        ));
        assert_eq!(p.len(), 1);
        a.finish(None);
        assert_eq!(p.len(), 0);
    }

    #[test]
    fn reads_proxy_header_only_when_trusted() {
        let mut h = HeaderMap::new();
        h.insert("x-forwarded-for", "1.2.3.4, 10.0.0.9".parse().unwrap());
        let peer = Some(A);
        assert_eq!(Public::new(cfg()).client_ip(&h, peer), A);
        let trusted = Public::new(PublicConfig {
            trust_proxy: true,
            ..cfg()
        });
        assert_eq!(
            trusted.client_ip(&h, peer),
            "10.0.0.9".parse::<IpAddr>().unwrap()
        );
        assert!(trusted.cfg.relay_allowed(" https://example.org/sparql"));
        assert!(!trusted.cfg.relay_allowed("http://169.254.169.254/"));
    }
}
