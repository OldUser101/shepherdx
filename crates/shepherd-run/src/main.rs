use tracing::{Level, error};

mod runner;
mod usercode;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_timer(tracing_subscriber::fmt::time::uptime())
        .with_max_level(Level::DEBUG)
        .init();

    let now = chrono::Local::now();
    tracing::info!("shepherd-run started at {}", now.to_rfc3339());

    match runner::Runner::new().await {
        Ok(mut r) => {
            if let Err(e) = r.run().await {
                error!("runner error: {e}");
            }
        }
        Err(e) => error!("failed to create runner: {e}"),
    }
}
