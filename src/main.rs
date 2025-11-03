use axum::{
    routing::{get, post},
    Router,
};
use log::info;
use std::{
    env,
    sync::Arc,
    time::Duration,
};
use tokio::sync::RwLock;

// Import our modules
mod constants;
mod handlers;
mod models;
mod services;
mod utils;

use models::{App, CircuitBreakerState};
use services::model_cache::refresh_models_cache;

#[tokio::main]
async fn main() {
    let _ = dotenvy::dotenv();

    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let backend_url = env::var("BACKEND_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:8000/v1/chat/completions".into());
    let backend_timeout_secs = env::var("BACKEND_TIMEOUT_SECS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(600);
    let circuit_breaker_enabled = env::var("ENABLE_CIRCUIT_BREAKER")
        .ok()
        .and_then(|s| s.parse::<bool>().ok())
        .unwrap_or(false);

    info!("🚀 Claude-to-OpenAI Proxy starting...");
    info!("   Backend URL: {}", backend_url);
    info!("   Backend Timeout: {}s", backend_timeout_secs);
    info!("   Circuit Breaker: {}", if circuit_breaker_enabled { "enabled" } else { "disabled" });
    info!("   Mode: Passthrough with case-correction");

    let models_cache = Arc::new(RwLock::new(None));
    let circuit_breaker = Arc::new(RwLock::new(CircuitBreakerState::new(circuit_breaker_enabled)));

    let app = App {
        client: reqwest::Client::builder()
            .pool_max_idle_per_host(1024)
            .tcp_keepalive(Some(Duration::from_secs(60)))
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(backend_timeout_secs))
            .build()
            .unwrap(),
        backend_url: backend_url.clone(),
        models_cache: models_cache.clone(),
        circuit_breaker: circuit_breaker.clone(),
    };

    // Background model cache refresh (every 60s)
    {
        let app_clone = app.clone();
        tokio::spawn(async move {
            loop {
                if let Err(e) = refresh_models_cache(&app_clone).await {
                    log::warn!("Failed to refresh models cache: {}", e);
                }
                tokio::time::sleep(Duration::from_secs(60)).await;
            }
        });
    }

    let router = Router::new()
        .route("/health", get(handlers::health_check))
        .route("/v1/messages", post(handlers::messages))
        .route("/v1/messages/count_tokens", post(handlers::count_tokens))
        .layer(axum::extract::DefaultBodyLimit::max(10 * 1024 * 1024)) // 10MB limit
        .layer(tower_http::compression::CompressionLayer::new())
        .with_state(app);

    let port = env::var("HOST_PORT")
        .unwrap_or_else(|_| "8080".into())
        .parse::<u16>()
        .unwrap_or(8080);
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port))
        .await
        .unwrap();
    info!("   Listening on: 0.0.0.0:{}", port);
    axum::serve(listener, router).await.unwrap();
}