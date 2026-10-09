//! HTTP routes for the shacl-report web viewer.

use super::{AppState, assets};
use crate::report::{ReportInputs, load_report};
use axum::Json;
use axum::Router;
use axum::body::Body;
use axum::extract::State;
use axum::http::{StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(root))
        .route("/assets/*path", get(asset))
        .route("/api/meta", get(meta))
        .route("/api/report", get(report))
        .route("/api/load", post(load))
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
