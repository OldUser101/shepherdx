use anyhow::Result;
use axum::Router;
use shepherd_mqtt::MqttClient;
use tokio::net::TcpListener;
use tower::ServiceBuilder;
use tower_http::{services::fs::ServeDir, trace::TraceLayer};
use tracing::{Level, warn};

mod control;
mod error;
mod files;
mod upload;

const STATIC_DIR: &str = "/var/shepherd/static";
const SERVICE_ID: &str = "shepherd-app";

async fn _main() -> Result<()> {
    let (client, mut event_loop) =
        MqttClient::new(SERVICE_ID.to_string(), "localhost".to_string(), 1883);

    let app = Router::new()
        .nest("/control", control::router(client))
        .nest("/files", files::router())
        .nest("/upload", upload::router())
        .fallback_service(ServiceBuilder::new().service(ServeDir::new(STATIC_DIR)))
        .layer(TraceLayer::new_for_http());
    let listener = TcpListener::bind("0.0.0.0:8080").await?;

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
