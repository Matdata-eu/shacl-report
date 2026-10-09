//! HTTP routes for the shacl-report web viewer.

use super::{AppState, assets};
use crate::input::MemoryFile;
use crate::report::{ReportInputs, load_report, load_report_from_memory};
use axum::Json;
use axum::Router;
use axum::body::Body;
use axum::extract::{DefaultBodyLimit, State};
use axum::http::{StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;

/// Upper bound for one upload request (base64-encoded files in JSON).
const MAX_UPLOAD_BYTES: usize = 1024 * 1024 * 1024;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(root))
        .route("/assets/*path", get(asset))
        .route("/api/meta", get(meta))
        .route("/api/report", get(report))
        .route("/api/export.html", get(export_html))
        .route("/api/load", post(load))
        .route(
            "/api/upload",
            post(upload).layer(DefaultBodyLimit::max(MAX_UPLOAD_BYTES)),
        )
        .with_state(state)
}

async fn root() -> Response {
    match assets::lookup("/assets/app/index.html") {
        Some(a) => Html(std::str::from_utf8(a.bytes).unwrap_or("")).into_response(),
        None => (StatusCode::NOT_FOUND, "missing").into_response(),
    }
}

async fn asset(axum::extract::Path(path): axum::extract::Path<String>) -> Response {
    let full = format!("/assets/{path}");
    match assets::lookup(&full) {
        Some(a) => {
            let mut resp = Response::new(Body::from(a.bytes));
            resp.headers_mut()
                .insert(header::CONTENT_TYPE, a.mime.parse().unwrap());
            resp
        }
        None => (StatusCode::NOT_FOUND, "not found").into_response(),
    }
}

#[derive(Serialize)]
struct MetaDto {
    version: &'static str,
    loaded: bool,
}

async fn meta(State(s): State<AppState>) -> Json<MetaDto> {
    Json(MetaDto {
        version: env!("CARGO_PKG_VERSION"),
        loaded: s.data.lock().await.is_some(),
    })
}

async fn report(State(s): State<AppState>) -> Response {
    let Some(data) = s.data.lock().await.clone() else {
        return (StatusCode::CONFLICT, "no report loaded").into_response();
    };
    match tokio::task::spawn_blocking(move || serde_json::to_vec(&*data)).await {
        Ok(Ok(bytes)) => {
            let mut resp = Response::new(Body::from(bytes));
            resp.headers_mut()
                .insert(header::CONTENT_TYPE, "application/json".parse().unwrap());
            resp
        }
        Ok(Err(e)) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "task panic").into_response(),
    }
}

async fn export_html(State(s): State<AppState>) -> Response {
    let Some(data) = s.data.lock().await.clone() else {
        return (StatusCode::CONFLICT, "no report loaded").into_response();
    };
    match tokio::task::spawn_blocking(move || super::export::render_html(&data)).await {
        Ok(Ok(html)) => {
            let mut resp = Response::new(Body::from(html));
            let headers = resp.headers_mut();
            headers.insert(
                header::CONTENT_TYPE,
                "text/html; charset=utf-8".parse().unwrap(),
            );
            headers.insert(
                header::CONTENT_DISPOSITION,
                "attachment; filename=\"shacl-report.html\""
                    .parse()
                    .unwrap(),
            );
            resp
        }
        Ok(Err(e)) => (StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}")).into_response(),
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "task panic").into_response(),
    }
}

#[derive(Deserialize)]
struct LoadBody {
    report: PathBuf,
    #[serde(default)]
    data: Vec<PathBuf>,
    #[serde(default)]
    shapes: Vec<PathBuf>,
}

async fn load(State(s): State<AppState>, Json(body): Json<LoadBody>) -> Response {
    let inputs = ReportInputs {
        report: body.report,
        data: body.data,
        shapes: body.shapes,
        format: None,
    };
    let result = match tokio::task::spawn_blocking(move || load_report(&inputs)).await {
        Ok(r) => r,
        Err(e) => Err(anyhow::anyhow!("task panic: {e}")),
    };
    match result {
        Ok(r) => {
            *s.data.lock().await = Some(Arc::new(r));
            (StatusCode::OK, "ok").into_response()
        }
        Err(e) => (StatusCode::BAD_REQUEST, format!("{e:#}")).into_response(),
    }
}

#[derive(Deserialize)]
struct UploadFile {
    name: String,
    /// Base64-encoded file content.
    content: String,
}

#[derive(Deserialize)]
struct UploadBody {
    report: UploadFile,
    #[serde(default)]
    data: Vec<UploadFile>,
    #[serde(default)]
    shapes: Vec<UploadFile>,
}

fn decode(f: UploadFile) -> anyhow::Result<MemoryFile> {
    use base64::Engine as _;
    let content = base64::engine::general_purpose::STANDARD
        .decode(f.content.as_bytes())
        .map_err(|e| anyhow::anyhow!("{}: invalid upload encoding: {e}", f.name))?;
    Ok(MemoryFile {
        name: f.name,
        content,
    })
}

async fn upload(State(s): State<AppState>, Json(body): Json<UploadBody>) -> Response {
    let result = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let report = decode(body.report)?;
        let shapes = body
            .shapes
            .into_iter()
            .map(decode)
            .collect::<anyhow::Result<Vec<_>>>()?;
        let data = body
            .data
            .into_iter()
            .map(decode)
            .collect::<anyhow::Result<Vec<_>>>()?;
        load_report_from_memory(&report, &shapes, &data)
    })
    .await
    .unwrap_or_else(|e| Err(anyhow::anyhow!("task panic: {e}")));
    match result {
        Ok(r) => {
            *s.data.lock().await = Some(Arc::new(r));
            (StatusCode::OK, "ok").into_response()
        }
        Err(e) => (StatusCode::BAD_REQUEST, format!("{e:#}")).into_response(),
    }
}
