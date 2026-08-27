//! axum HTTP server: serves the embedded React dashboard, a `/ws` endpoint
//! that broadcasts a metrics snapshot every 500ms, and a Prometheus text
//! exposition endpoint at `/metrics`.

use crate::metrics::{Metrics, MetricsPayload, TopKey};
use axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    extract::State,
    http::{header, StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use persistence::Engine;
use rust_embed::RustEmbed;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::broadcast;

#[derive(RustEmbed)]
#[folder = "../../dashboard/dist"]
struct Dashboard;

struct AppState {
    engine: Arc<Engine>,
    metrics: Arc<Metrics>,
    tx: broadcast::Sender<String>,
}

pub async fn run(
    addr: SocketAddr,
    engine: Arc<Engine>,
    metrics: Arc<Metrics>,
) -> std::io::Result<()> {
    let (tx, _rx) = broadcast::channel::<String>(32);
    spawn_broadcaster(engine.clone(), metrics.clone(), tx.clone());

    let state = Arc::new(AppState {
        engine,
        metrics,
        tx,
    });

    let app = Router::new()
        .route("/ws", get(ws_handler))
        .route("/metrics", get(prometheus_handler))
        .fallback(static_handler)
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(%addr, "HTTP dashboard listening");
    axum::serve(listener, app).await
}

fn spawn_broadcaster(engine: Arc<Engine>, metrics: Arc<Metrics>, tx: broadcast::Sender<String>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(500));
        let mut last_ops = metrics.total_ops();
        let mut last_tick = Instant::now();
        loop {
            interval.tick().await;
            if tx.receiver_count() == 0 {
                // Nobody's listening; still advance the window so the next
                // ops/sec reading isn't inflated by the idle gap.
                last_ops = metrics.total_ops();
                last_tick = Instant::now();
                continue;
            }

            let now = Instant::now();
            let elapsed = now.duration_since(last_tick).as_secs_f64().max(0.001);
            let cur_ops = metrics.total_ops();
            let ops_per_sec = (cur_ops.saturating_sub(last_ops)) as f64 / elapsed;
            last_ops = cur_ops;
            last_tick = now;

            let stats = engine.stats();
            let top_keys = metrics
                .top_keys(10)
                .into_iter()
                .map(|(k, count)| TopKey {
                    key: String::from_utf8_lossy(&k).into_owned(),
                    count,
                })
                .collect();

            let payload = MetricsPayload {
                total_ops: cur_ops,
                ops_per_sec,
                memory_bytes: stats.arena_capacity_bytes as u64,
                memory_live_bytes: stats.arena_live_bytes as u64,
                key_count: stats.len as u64,
                top_keys,
            };
            if let Ok(json) = serde_json::to_string(&payload) {
                let _ = tx.send(json);
            }
        }
    });
}

async fn ws_handler(State(state): State<Arc<AppState>>, ws: WebSocketUpgrade) -> Response {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(mut socket: WebSocket, state: Arc<AppState>) {
    let mut rx = state.tx.subscribe();
    loop {
        tokio::select! {
            msg = rx.recv() => {
                match msg {
                    Ok(json) => {
                        if socket.send(Message::Text(json)).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            incoming = socket.recv() => {
                match incoming {
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Err(_)) => break,
                    _ => {}
                }
            }
        }
    }
}

async fn prometheus_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let stats = state.engine.stats();
    let total_ops = state.metrics.total_ops();
    let mut out = String::new();
    out.push_str("# HELP kvdb_total_ops Total number of operations processed\n");
    out.push_str("# TYPE kvdb_total_ops counter\n");
    out.push_str(&format!("kvdb_total_ops {total_ops}\n"));
    out.push_str("# HELP kvdb_keys_total Number of keys currently stored\n");
    out.push_str("# TYPE kvdb_keys_total gauge\n");
    out.push_str(&format!("kvdb_keys_total {}\n", stats.len));
    out.push_str(
        "# HELP kvdb_memory_bytes Arena capacity in bytes (includes freed-but-unreleased space)\n",
    );
    out.push_str("# TYPE kvdb_memory_bytes gauge\n");
    out.push_str(&format!(
        "kvdb_memory_bytes {}\n",
        stats.arena_capacity_bytes
    ));
    out.push_str("# HELP kvdb_memory_live_bytes Live (non-freed) arena bytes\n");
    out.push_str("# TYPE kvdb_memory_live_bytes gauge\n");
    out.push_str(&format!(
        "kvdb_memory_live_bytes {}\n",
        stats.arena_live_bytes
    ));
    out.push_str("# HELP kvdb_node_count Number of B-Tree nodes in use\n");
    out.push_str("# TYPE kvdb_node_count gauge\n");
    out.push_str(&format!("kvdb_node_count {}\n", stats.node_count));

    ([(header::CONTENT_TYPE, "text/plain; version=0.0.4")], out)
}

async fn static_handler(uri: Uri) -> Response {
    let mut path = uri.path().trim_start_matches('/').to_string();
    if path.is_empty() {
        path = "index.html".to_string();
    }
    if let Some(content) = Dashboard::get(&path) {
        return (
            StatusCode::OK,
            [(header::CONTENT_TYPE, mime_for(&path))],
            content.data,
        )
            .into_response();
    }
    // SPA fallback: unknown routes serve index.html so client-side routing works.
    if let Some(content) = Dashboard::get("index.html") {
        return (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
            content.data,
        )
            .into_response();
    }
    (StatusCode::NOT_FOUND, "dashboard not built").into_response()
}

fn mime_for(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        _ => "application/octet-stream",
    }
}
