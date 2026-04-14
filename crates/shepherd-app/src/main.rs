use std::time::Duration;

use axum::Router;
use rumqttc::{AsyncClient, MqttOptions};
use tokio::net::TcpListener;
use tower_http::trace::TraceLayer;
use tracing::Level;

mod error;
mod files;
mod upload;

const SERVICE_ID: &str = "shepherd-app";

async fn _main() -> Result<(), Box<dyn std::error::Error>> {
    let app = Router::new()
        .nest("/files", files::router())
        .nest("/upload", upload::router())
        .layer(TraceLayer::new_for_http());

    let mut mqttoptions = MqttOptions::new(SERVICE_ID, "localhost", 1883);
    mqttoptions.set_keep_alive(Duration::from_secs(5));

    let (mut mqttc, mut eventloop) = AsyncClient::new(mqttoptions, 10);

    let listener = TcpListener::bind("0.0.0.0:8080").await?;

    let (r0, r1) = tokio::join!(axum::serve(listener, app), eventloop.poll());

    r0?;
    r1?;

    Ok(())
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_timer(tracing_subscriber::fmt::time::uptime())
        .with_max_level(Level::INFO)
        .init();

    let now = chrono::Local::now();
    tracing::info!("shepherd-app started at {}", now.to_rfc3339());

    if let Err(e) = _main().await {
        tracing::error!("{}", e.to_string());
        std::process::exit(1);
    }
}
