//! Health check HTTP server for Kubernetes probes
//!
//! Provides:
//! - `/health` - Liveness probe (always returns 200 if server is running)
//! - `/ready` - Readiness probe (returns 200 when app is ready to serve traffic)
//! - `/metrics` - Basic metrics endpoint with JSON stats

use axum::{extract::State, http::StatusCode, response::IntoResponse, routing::get, Json, Router};
use serde::Serialize;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tokio::net::TcpListener;
use tracing::{error, info};

/// Default port for health check server
pub const DEFAULT_HEALTH_PORT: u16 = 8080;

/// Shared application state for health checks
#[derive(Debug)]
pub struct HealthState {
    /// Whether workers have been initialized
    pub workers_ready: AtomicBool,
    /// Whether gas prices have been fetched
    pub gas_prices_ready: AtomicBool,
    /// Application start time
    pub start_time: Instant,
    /// Total transactions processed
    pub transactions_processed: AtomicU64,
    /// Total reactions sent
    pub reactions_sent: AtomicU64,
    /// Last successful block fetch timestamp (unix epoch millis)
    pub last_block_fetch_ms: AtomicU64,
}

impl Default for HealthState {
    fn default() -> Self {
        Self::new()
    }
}

impl HealthState {
    /// Create new health state
    pub fn new() -> Self {
        Self {
            workers_ready: AtomicBool::new(false),
            gas_prices_ready: AtomicBool::new(false),
            start_time: Instant::now(),
            transactions_processed: AtomicU64::new(0),
            reactions_sent: AtomicU64::new(0),
            last_block_fetch_ms: AtomicU64::new(0),
        }
    }

    /// Mark workers as ready
    pub fn set_workers_ready(&self, ready: bool) {
        self.workers_ready.store(ready, Ordering::SeqCst);
    }

    /// Mark gas prices as ready
    pub fn set_gas_prices_ready(&self, ready: bool) {
        self.gas_prices_ready.store(ready, Ordering::SeqCst);
    }

    /// Check if app is fully ready (gas prices fetched AND workers initialized)
    pub fn is_ready(&self) -> bool {
        self.gas_prices_ready.load(Ordering::SeqCst) && self.workers_ready.load(Ordering::SeqCst)
    }

    /// Increment transactions processed counter
    pub fn inc_transactions(&self) {
        self.transactions_processed.fetch_add(1, Ordering::Relaxed);
    }

    /// Increment reactions sent counter
    pub fn inc_reactions(&self) {
        self.reactions_sent.fetch_add(1, Ordering::Relaxed);
    }

    /// Update last block fetch time
    pub fn update_last_block_fetch(&self) {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        self.last_block_fetch_ms.store(now_ms, Ordering::Relaxed);
    }
}

/// Response for liveness probe
#[derive(Serialize)]
struct LivenessResponse {
    status: &'static str,
    uptime_secs: u64,
}

/// Response for readiness probe
#[derive(Serialize)]
struct ReadinessResponse {
    status: &'static str,
    workers_ready: bool,
    gas_prices_ready: bool,
}

/// Response for metrics endpoint
#[derive(Serialize)]
struct MetricsResponse {
    uptime_secs: u64,
    workers_ready: bool,
    gas_prices_ready: bool,
    transactions_processed: u64,
    reactions_sent: u64,
    last_block_fetch_ms: u64,
}

/// Liveness probe handler - returns 200 if server is running
async fn liveness(State(state): State<Arc<HealthState>>) -> impl IntoResponse {
    let response = LivenessResponse {
        status: "alive",
        uptime_secs: state.start_time.elapsed().as_secs(),
    };
    (StatusCode::OK, Json(response))
}

/// Readiness probe handler - returns 200 only when app is ready
async fn readiness(State(state): State<Arc<HealthState>>) -> impl IntoResponse {
    let workers_ready = state.workers_ready.load(Ordering::SeqCst);
    let gas_prices_ready = state.gas_prices_ready.load(Ordering::SeqCst);

    let response = ReadinessResponse {
        status: if state.is_ready() {
            "ready"
        } else {
            "not_ready"
        },
        workers_ready,
        gas_prices_ready,
    };

    if state.is_ready() {
        (StatusCode::OK, Json(response))
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, Json(response))
    }
}

/// Metrics endpoint - returns application statistics
async fn metrics(State(state): State<Arc<HealthState>>) -> impl IntoResponse {
    let response = MetricsResponse {
        uptime_secs: state.start_time.elapsed().as_secs(),
        workers_ready: state.workers_ready.load(Ordering::SeqCst),
        gas_prices_ready: state.gas_prices_ready.load(Ordering::SeqCst),
        transactions_processed: state.transactions_processed.load(Ordering::Relaxed),
        reactions_sent: state.reactions_sent.load(Ordering::Relaxed),
        last_block_fetch_ms: state.last_block_fetch_ms.load(Ordering::Relaxed),
    };
    (StatusCode::OK, Json(response))
}

/// Start the health check HTTP server
///
/// This runs in the background and provides:
/// - GET /health - Liveness probe
/// - GET /ready - Readiness probe
/// - GET /metrics - Application metrics
/// - GET /api/* - Dashboard API (if dashboard_state provided)
/// - GET /* - Dashboard frontend static files (fallback)
pub async fn start_health_server(
    state: Arc<HealthState>,
    dashboard_state: Option<Arc<crate::DashboardState>>,
    port: u16,
) {
    let health_routes = Router::new()
        .route("/health", get(liveness))
        .route("/ready", get(readiness))
        .route("/metrics", get(metrics))
        .with_state(state);

    let app = if let Some(ds) = dashboard_state {
        health_routes.merge(crate::dashboard::api::dashboard_router(ds))
    } else {
        health_routes
    };

    let addr = SocketAddr::from(([0, 0, 0, 0], port));

    info!("Health server starting on http://{}", addr);

    let listener = match TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            error!("Failed to bind health server to {}: {}", addr, e);
            return;
        }
    };

    if let Err(e) = axum::serve(listener, app).await {
        error!("Health server error: {}", e);
    }
}

/// Create the health router for testing
pub fn create_health_router(state: Arc<HealthState>) -> Router {
    Router::new()
        .route("/health", get(liveness))
        .route("/ready", get(readiness))
        .route("/metrics", get(metrics))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    #[test]
    fn test_health_state_default() {
        let state = HealthState::new();
        assert!(!state.workers_ready.load(Ordering::SeqCst));
        assert!(!state.gas_prices_ready.load(Ordering::SeqCst));
        assert!(!state.is_ready());
    }

    #[test]
    fn test_health_state_ready() {
        let state = HealthState::new();
        state.set_workers_ready(true);
        assert!(!state.is_ready()); // Still not ready without gas prices

        state.set_gas_prices_ready(true);
        assert!(state.is_ready()); // Now ready
    }

    #[test]
    fn test_health_state_counters() {
        let state = HealthState::new();
        assert_eq!(state.transactions_processed.load(Ordering::Relaxed), 0);

        state.inc_transactions();
        state.inc_transactions();
        assert_eq!(state.transactions_processed.load(Ordering::Relaxed), 2);

        state.inc_reactions();
        assert_eq!(state.reactions_sent.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn test_health_endpoint_returns_200() {
        let state = Arc::new(HealthState::new());
        let app = create_health_router(state);

        let request = Request::builder()
            .uri("/health")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["status"], "alive");
        assert!(json["uptime_secs"].as_u64().is_some());
    }

    #[tokio::test]
    async fn test_ready_endpoint_returns_503_when_not_ready() {
        let state = Arc::new(HealthState::new());
        let app = create_health_router(state);

        let request = Request::builder()
            .uri("/ready")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["status"], "not_ready");
        assert_eq!(json["workers_ready"], false);
        assert_eq!(json["gas_prices_ready"], false);
    }

    #[tokio::test]
    async fn test_ready_endpoint_returns_200_when_ready() {
        let state = Arc::new(HealthState::new());
        state.set_workers_ready(true);
        state.set_gas_prices_ready(true);

        let app = create_health_router(state);

        let request = Request::builder()
            .uri("/ready")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["status"], "ready");
        assert_eq!(json["workers_ready"], true);
        assert_eq!(json["gas_prices_ready"], true);
    }

    #[tokio::test]
    async fn test_metrics_endpoint() {
        let state = Arc::new(HealthState::new());
        state.set_workers_ready(true);
        state.inc_transactions();
        state.inc_transactions();
        state.inc_reactions();

        let app = create_health_router(state);

        let request = Request::builder()
            .uri("/metrics")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["workers_ready"], true);
        assert_eq!(json["gas_prices_ready"], false);
        assert_eq!(json["transactions_processed"], 2);
        assert_eq!(json["reactions_sent"], 1);
        assert!(json["uptime_secs"].as_u64().is_some());
    }
}
