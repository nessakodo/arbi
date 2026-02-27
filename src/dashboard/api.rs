use std::convert::Infallible;
use std::sync::Arc;

use axum::extract::{Query, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use tokio_stream::wrappers::WatchStream;
use tokio_stream::StreamExt;
use tower_http::services::ServeDir;

use super::state::DashboardState;

#[derive(serde::Deserialize)]
pub struct LimitParams {
    pub limit: Option<usize>,
}

/// GET /api/snapshot -- current state
async fn get_snapshot(State(state): State<Arc<DashboardState>>) -> impl IntoResponse {
    Json(state.current_snapshot())
}

/// GET /api/opportunities?limit=50 -- opportunity history
async fn get_opportunities(
    State(state): State<Arc<DashboardState>>,
    Query(params): Query<LimitParams>,
) -> impl IntoResponse {
    let limit = params.limit.unwrap_or(50).min(500);
    Json(state.get_opportunities(limit))
}

/// GET /api/pnl?limit=100 -- P&L history
async fn get_pnl(
    State(state): State<Arc<DashboardState>>,
    Query(params): Query<LimitParams>,
) -> impl IntoResponse {
    let limit = params.limit.unwrap_or(100).min(200);
    Json(state.get_pnl_history(limit))
}

/// GET /api/events -- SSE stream of snapshot updates
async fn sse_events(
    State(state): State<Arc<DashboardState>>,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>> {
    let stream = WatchStream::new(state.subscribe()).map(|snapshot| {
        let data = serde_json::to_string(&snapshot).unwrap_or_default();
        Ok(Event::default().data(data).event("snapshot"))
    });
    Sse::new(stream).keep_alive(KeepAlive::default())
}

/// Build the dashboard router with API routes + static file fallback
pub fn dashboard_router(state: Arc<DashboardState>) -> Router {
    Router::new()
        .route("/api/snapshot", get(get_snapshot))
        .route("/api/opportunities", get(get_opportunities))
        .route("/api/pnl", get(get_pnl))
        .route("/api/events", get(sse_events))
        .fallback_service(ServeDir::new("dashboard/dist"))
        .with_state(state)
}
