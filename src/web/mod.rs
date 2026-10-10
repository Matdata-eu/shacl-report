//! Local web viewer and self-contained HTML export.

pub mod api;
pub mod assets;
pub mod export;
pub mod public;

use crate::report::{Report, ReportInputs, load_report};
use anyhow::{Context, Result};
use public::{Public, PublicConfig};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Shared application state.
#[derive(Clone, Default)]
pub struct AppState {
    /// The report on screen. On a public server this is only the report
    /// preloaded at startup, shown to visitors who have not loaded their own.
    pub data: Arc<Mutex<Option<Arc<Report>>>>,
    /// SPARQL endpoint given on the command line, preset in the viewer.
    pub endpoint: Option<String>,
    /// Set when serving many users; see [`public`].
    pub public: Option<Arc<Public>>,
}

/// Server lifecycle wrapper.
pub struct Server {
    pub addr: SocketAddr,
    handle: tokio::task::JoinHandle<()>,
}

impl Server {
    pub async fn join(self) -> Result<()> {
        self.handle.await.context("web server task panicked")?;
        Ok(())
    }
}

/// What the viewer should show when it starts.
pub enum Preload {
    None,
    Files(ReportInputs),
    Loaded(Box<Report>),
}

pub async fn build_state(
    preload: Preload,
    endpoint: Option<String>,
    public: Option<PublicConfig>,
) -> Result<AppState> {
    let state = AppState {
        endpoint: endpoint.clone(),
        public: public.map(Public::new),
        ..AppState::default()
    };
    let report = match preload {
        Preload::None => None,
        Preload::Loaded(r) => Some(*r),
        Preload::Files(inputs) => Some(
            tokio::task::spawn_blocking(move || load_report(&inputs))
                .await
                .context("load task panicked")??,
        ),
    };
    *state.data.lock().await = report.map(|mut r| {
        r.sparql_endpoint = endpoint;
        Arc::new(r)
    });
    Ok(state)
}

/// Start the HTTP server. Returns once the listener is bound.
pub async fn start(bind: &str, state: AppState) -> Result<Server> {
    let listener = tokio::net::TcpListener::bind(bind)
        .await
        .with_context(|| format!("failed to bind {bind}"))?;
    let addr = listener.local_addr().context("local_addr")?;
    if let Some(public) = state.public.clone() {
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(std::time::Duration::from_secs(60));
            loop {
                tick.tick().await;
                public.sweep();
            }
        });
    }
    let app = api::router(state).into_make_service_with_connect_info::<SocketAddr>();
    let handle = tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            eprintln!("web server error: {e}");
        }
    });
    Ok(Server { addr, handle })
}

/// Synchronous helper used by `main.rs`. Builds a current-thread Tokio runtime,
/// starts the server, optionally opens the browser, and blocks until ctrl-c
/// (or SIGTERM, as sent by `docker stop`).
pub fn run_blocking(
    bind: &str,
    open: bool,
    preload: Preload,
    endpoint: Option<String>,
    public: Option<PublicConfig>,
) -> Result<()> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("failed to build tokio runtime")?;
    rt.block_on(async move {
        let is_public = public.is_some();
        let state = build_state(preload, endpoint, public).await?;
        if let Some(p) = &state.public {
            let c = &p.cfg;
            eprintln!(
                "public mode: max {} sessions ({} per IP), idle timeout {} min, {} parallel jobs, uploads up to {} MB / {} triples",
                c.max_sessions,
                c.max_sessions_per_ip,
                c.session_idle.as_secs() / 60,
                c.max_jobs,
                c.max_upload_bytes / 1024 / 1024,
                c.max_triples
            );
        }
        let server = start(bind, state).await?;
        let url = format!("http://{}/", server.addr);
        eprintln!("shacl-report viewer listening on {url}");
        let open = open && !is_public;
        if open && let Err(e) = webbrowser::open(&url) {
            eprintln!("could not open browser: {e}");
        }
        tokio::select! {
            r = shutdown_signal() => {
                r?;
                eprintln!("shutting down");
                Ok::<(), anyhow::Error>(())
            }
            _ = server.handle => Ok(()),
        }
    })
}

/// Resolves on ctrl-c, or on SIGTERM where there is one.
async fn shutdown_signal() -> Result<()> {
    #[cfg(unix)]
    {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .context("SIGTERM handler failed")?;
        tokio::select! {
            r = tokio::signal::ctrl_c() => r.context("ctrl-c handler failed"),
            _ = term.recv() => Ok(()),
        }
    }
    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c()
            .await
            .context("ctrl-c handler failed")
    }
}
