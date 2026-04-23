use anyhow::Result;
use bytes::{Bytes, BytesMut};
use futures_util::{SinkExt, StreamExt};
use hopper::{Pipe, PipeMode};
use shepherd_common::config::Config;
use shepherd_mqtt::MqttClient;
use tokio::{
    net::{TcpListener, TcpStream},
    sync::broadcast::{self, Receiver, Sender},
};
use tokio_tungstenite::{
    accept_hdr_async,
    tungstenite::{
        Message,
        handshake::server::{Request, Response},
    },
};
use tracing::{Level, error, info, warn};

const HOPPER_BUF_SIZE: usize = 65536;

async fn handle_websocket_connection(
    stream: TcpStream,
    rx: Receiver<(String, Bytes)>,
) -> Result<()> {
    let mut rx = rx;
    let mut sub_topic: Option<String> = None;

    #[allow(clippy::result_large_err)]
    let callback = |req: &Request, resp: Response| {
        sub_topic = Some(req.uri().to_string().split_at(1).1.to_string());
        Ok(resp)
    };

    let addr = stream.peer_addr()?;
    let (mut ws_tx, _) = accept_hdr_async(stream, callback).await?.split();

    info!("subscription from {:?}, topic {:?}", addr, sub_topic);

    while let Ok((topic, payload)) = rx.recv().await {
        if let Some(sub_topic) = &sub_topic
            && *sub_topic == topic
        {
            ws_tx.send(Message::Binary(payload.clone())).await?;
        }
    }

    Ok(())
}

async fn handle_mqtt_message(
    sender: Sender<(String, Bytes)>,
    topic: String,
    msg: Bytes,
) -> Result<()> {
    // broadcast everywhere, result doesn't matter much
    let _ = sender.send((topic, msg));
    Ok(())
}

fn dispatch_log_messages(
    log_pipe: Pipe,
    sender: Sender<(String, Bytes)>,
    topic: String,
) -> Result<()> {
    let mut buf = BytesMut::with_capacity(HOPPER_BUF_SIZE);

    loop {
        buf.resize(HOPPER_BUF_SIZE, 0);

        match log_pipe.read(&mut buf) {
            Ok(n) => {
                buf.truncate(n);
                let _ = sender.send((topic.clone(), buf.clone().into()));
            }
            Err(e) => {
                error!("log error: {e}");
            }
        }
    }
}

async fn _main() -> Result<()> {
    let config = Config::from_file(None).unwrap_or_default();
    config.setup_dirs()?;

    let mut log_pipe = Pipe::new(
        PipeMode::OUT,
        &config.ws.service_id,
        &config.channel.robot_log,
        Some(config.path.hopper.clone()),
    )?;
    log_pipe.open()?;

    let (mut mqtt_client, mut mqtt_event_loop) = MqttClient::new(
        &config.run.service_id,
        &config.mqtt.broker,
        config.mqtt.port,
    );

    // run mqtt event loop independently
    let mqtt_loop = tokio::spawn(async move {
        // run mqtt forever, it "should" reconnect
        loop {
            if let Err(e) = mqtt_event_loop.run().await {
                error!("mqtt loop exited: {e}");
            }
        }
    });

    // broadcasting for messages to websockets
    let (msg_sender, _) = broadcast::channel::<(String, Bytes)>(64);

    let mqtt_sender = msg_sender.clone();
    mqtt_client
        .subscribe_raw("#", move |t, v| {
            let mqtt_sender = mqtt_sender.clone();
            async move { handle_mqtt_message(mqtt_sender, t, v).await }
        })
        .await?;

    // dispatch log messages forever
    let log_sender = msg_sender.clone();
    let log_topic = config.channel.robot_log.clone();
    tokio::task::spawn_blocking(move || {
        if let Err(e) = dispatch_log_messages(log_pipe, log_sender, log_topic) {
            error!("log dispatch: {e}");
        }
    });

    let listener = TcpListener::bind(format!("{}:{}", &config.ws.host, config.ws.port)).await?;

    tokio::select! {
        res = async {
            loop {
                match listener.accept().await {
                    Ok((stream, _)) => {
                        // spawn a task to handle this websocket, exists for websocket lifetime
                        tokio::spawn(handle_websocket_connection(stream, msg_sender.subscribe()));
                    },
                    Err(e) => return Err(e),
                }
            }
        } => {
            warn!("websocket handler exited {:?}", res);
            res?
        }

        _ = mqtt_loop => {
            error!("mqtt client exited?");
        }
    };

    Ok(())
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_timer(tracing_subscriber::fmt::time::uptime())
        .with_max_level(Level::DEBUG)
        .init();

    let now = chrono::Local::now();
    tracing::info!("shepherd-ws started at {}", now.to_rfc3339());

    if let Err(e) = _main().await {
        error!("relay error: {e}");
    }
}
