//! Local web viewer and self-contained HTML export.

pub mod api;
pub mod assets;
pub mod export;

use crate::report::{Report, ReportInputs, load_report};
use anyhow::{Context, Result};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Shared application state.
#[derive(Clone, Default)]
pub struct AppState {
    pub data: Arc<Mutex<Option<Arc<Report>>>>,
    /// SPARQL endpoint given on the command line, preset in the viewer.
    pub endpoint: Option<String>,
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

pub async fn build_state(preload: Preload, endpoint: Option<String>) -> Result<AppState> {
    let state = AppState {
        endpoint: endpoint.clone(),
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
    let app = api::router(state);
    let handle = tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            eprintln!("web server error: {e}");
        }
    });
    Ok(Server { addr, handle })
}

/// Synchronous helper used by `main.rs`. Builds a current-thread Tokio runtime,
/// starts the server, optionally opens the browser, and blocks until ctrl-c.
pub fn run_blocking(
    bind: &str,
    open: bool,
    preload: Preload,
    endpoint: Option<String>,
) -> Result<()> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("failed to build tokio runtime")?;
    rt.block_on(async move {
        let state = build_state(preload, endpoint).await?;
        let server = start(bind, state).await?;
        let url = format!("http://{}/", server.addr);
        eprintln!("shacl-report viewer listening on {url}");
        if open && let Err(e) = webbrowser::open(&url) {
            eprintln!("could not open browser: {e}");
        }
        tokio::select! {
            r = tokio::signal::ctrl_c() => {
                r.context("ctrl-c handler failed")?;
                eprintln!("shutting down");
                Ok::<(), anyhow::Error>(())
            }
            _ = server.handle => Ok(()),
        }
    })
}
