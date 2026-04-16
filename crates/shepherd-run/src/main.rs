use tracing::{Level, error};

mod runner;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_timer(tracing_subscriber::fmt::time::uptime())
        .with_max_level(Level::DEBUG)
        .init();

    let now = chrono::Local::now();
    tracing::info!("shepherd-run started at {}", now.to_rfc3339());

    let mut r = runner::Runner::new().await;
    if let Err(e) = r.run().await {
        error!("runner error: {e}");
    }
}
