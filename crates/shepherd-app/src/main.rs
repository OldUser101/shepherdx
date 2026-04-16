use anyhow::Result;
use axum::Router;
use shepherd_common::config::Config;
use shepherd_mqtt::MqttClient;
use tokio::net::TcpListener;
use tower::ServiceBuilder;
use tower_http::{services::fs::ServeDir, trace::TraceLayer};
use tracing::{Level, warn};

mod control;
mod error;
mod files;
mod upload;

async fn _main() -> Result<()> {
    let config = Config::from_file(None).unwrap_or_default();

    let (client, mut event_loop) = MqttClient::new(
        config.app.service_id.clone(),
        config.mqtt.broker.clone(),
        config.mqtt.port,
    );

    let app = Router::new()
        .nest("/control", control::router(&config, client))
        .nest("/files", files::router(&config))
        .nest("/upload", upload::router(&config))
        .fallback_service(ServiceBuilder::new().service(ServeDir::new(config.app.static_dir)))
        .layer(TraceLayer::new_for_http());
    let listener = TcpListener::bind(format!("{}:{}", config.app.host, config.app.port)).await?;

    tokio::select!(
        res = axum::serve(listener, app) => {
            warn!("server exited: {:#?}", res);
            res?
        }
        res = event_loop.run() => {
            warn!("mqtt client exited: {:#?}", res);
            res?
        }
    );

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
