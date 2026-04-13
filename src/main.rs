use axum::Router;
use tokio::net::TcpListener;
use tower_http::trace::TraceLayer;
use tracing::{Level, info};

mod error;
mod files;
mod upload;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_timer(tracing_subscriber::fmt::time::uptime())
        .with_max_level(Level::INFO)
        .init();

    let now = chrono::Local::now();
    info!("shepherd-app started at {}", now.to_rfc3339());

    let app = Router::new()
        .nest("/files", files::router())
        .nest("/upload", upload::router())
        .layer(TraceLayer::new_for_http());

    let listener = TcpListener::bind("0.0.0.0:8080").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
