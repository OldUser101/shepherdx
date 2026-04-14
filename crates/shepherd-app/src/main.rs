use anyhow::Result;
use axum::Router;
use shepherd_mqtt::MqttClient;
use tokio::net::TcpListener;
use tower_http::trace::TraceLayer;
use tracing::Level;

mod error;
mod files;
mod upload;

const SERVICE_ID: &str = "shepherd-app";

async fn _main() -> Result<()> {
    let mut mqttc = MqttClient::new(SERVICE_ID.to_string(), "localhost".to_string(), 1883);

    let app = Router::new()
        .nest("/files", files::router())
        .nest("/upload", upload::router())
        .layer(TraceLayer::new_for_http());
    let listener = TcpListener::bind("0.0.0.0:8080").await?;

    let (r0, r1) = tokio::join!(axum::serve(listener, app), mqttc.run());

    // this is actually horrid
    r0?;
    r1?;

    Ok(())
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_timer(tracing_subscriber::fmt::time::uptime())
        .with_max_level(Level::DEBUG)
        .init();

    let now = chrono::Local::now();
    tracing::info!("shepherd-app started at {}", now.to_rfc3339());

    if let Err(e) = _main().await {
        tracing::error!("{}", e.to_string());
        std::process::exit(1);
    }
}
