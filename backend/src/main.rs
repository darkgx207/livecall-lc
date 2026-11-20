mod protocol;
mod room_manager;
mod signaling;

use axum::{routing::get, Router};
use room_manager::RoomManager;
use std::{net::SocketAddr, sync::Arc};
use tower_http::trace::TraceLayer;
use tracing::info;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "livecall_backend=info,tower_http=info".into()),
        )
        .init();

    let manager = Arc::new(RoomManager::default());
    let app = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/ws", get(signaling::websocket_handler))
        .with_state(manager)
        .layer(TraceLayer::new_for_http());

    let address: SocketAddr = std::env::var("LIVECALL_BIND")
        .unwrap_or_else(|_| "0.0.0.0:3000".to_string())
        .parse()
        .expect("LIVECALL_BIND precisa ser um endereço válido, ex.: 127.0.0.1:3000");

    let listener = tokio::net::TcpListener::bind(address)
        .await
        .expect("não foi possível abrir a porta do LiveCall");
    info!(%address, "LiveCall signaling server iniciado");
    axum::serve(listener, app)
        .await
        .expect("erro ao executar servidor HTTP/WebSocket");
}
