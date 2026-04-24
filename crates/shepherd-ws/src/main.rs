use anyhow::Result;
use bytes::{Bytes, BytesMut};
use futures_util::{SinkExt, StreamExt};
use hopper::{Pipe, PipeMode};
use shepherd_common::config::Config;
use shepherd_mqtt::MqttClient;
use tokio::{
    net::{TcpListener, TcpStream},
    sync::{broadcast, watch},
};
use tokio_tungstenite::{
    accept_hdr_async,
    tungstenite::{
        Message,
        handshake::server::{Request, Response},
    },
};
use tracing::{Level, debug, error, info, warn};

const HOPPER_BUF_SIZE: usize = 65536;

#[derive(Debug)]
struct WsState {
    camera: String,
    cam_rx: watch::Receiver<(String, Bytes)>,
    msg_rx: broadcast::Receiver<(String, Bytes)>,
}

enum MessageReceiver {
    Raw(String, broadcast::Receiver<(String, Bytes)>),
    Image(String, watch::Receiver<(String, Bytes)>),
}

impl MessageReceiver {
    /// wait until a valid message is received on this channel
    pub async fn recv(&mut self) -> Result<Bytes> {
        loop {
            if let Some(res) = match self {
                Self::Raw(topic, rx) => match rx.recv().await {
                    Ok((rx_topic, payload)) => {
                        if rx_topic == *topic {
                            Some(Ok(payload))
                        } else {
                            None
                        }
                    }
                    Err(e) => {
                        if e == broadcast::error::RecvError::Closed {
                            Some(Err(anyhow::anyhow!("receive failed: {e}")))
                        } else {
                            None
                        }
                    }
                },

                Self::Image(topic, rx) => match rx.changed().await {
                    Ok(()) => {
                        let payload = rx.borrow_and_update().clone();
                        if payload.0 == *topic {
                            Some(Ok(payload.1))
                        } else {
                            None
                        }
                    }
                    Err(e) => Some(Err(anyhow::anyhow!("{e}"))),
                },
            } {
                return res;
            }
        }
    }
}

async fn handle_websocket_connection(stream: TcpStream, state: WsState) -> Result<()> {
    let mut sub_topic: Option<String> = None;

    #[allow(clippy::result_large_err)]
    let callback = |req: &Request, resp: Response| {
        sub_topic = Some(req.uri().to_string().split_at(1).1.to_string());
        Ok(resp)
    };

    let addr = stream.peer_addr()?;
    let (mut ws_tx, _) = accept_hdr_async(stream, callback).await?.split();

    let sub_topic = if let Some(sub_topic) = sub_topic {
        sub_topic
    } else {
        return Err(anyhow::anyhow!("websocket subscription topic not set"));
    };

    info!("subscription from {:?}, topic {:?}", addr, sub_topic);

    let mut rx = if sub_topic == state.camera {
        MessageReceiver::Image(state.camera, state.cam_rx)
    } else {
        MessageReceiver::Raw(sub_topic, state.msg_rx)
    };

    loop {
        match rx.recv().await {
            Ok(payload) => {
                ws_tx.send(Message::Binary(payload)).await?;
            }
            Err(e) => {
                return Err(e)?;
            }
        }
    }
}

/// forward raw mqtt messages to websockets
async fn handle_mqtt_message(
    sender: broadcast::Sender<(String, Bytes)>,
    topic: String,
    msg: Bytes,
) -> Result<()> {
    // broadcast everywhere, result doesn't matter much
    let _ = sender.send((topic, msg));
    Ok(())
}

/// read logs from hopper and push them to websockets
fn dispatch_log_messages(
    log_pipe: Pipe,
    sender: broadcast::Sender<(String, Bytes)>,
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

/// read images from hopper and push them to websockets
fn dispatch_images(
    camera_pipe: Pipe,
    sender: watch::Sender<(String, Bytes)>,
    topic: String,
) -> Result<()> {
    let mut buf = BytesMut::new();

    loop {
        let mut cbuf = BytesMut::with_capacity(HOPPER_BUF_SIZE);
        cbuf.resize(HOPPER_BUF_SIZE, 0);

        match camera_pipe.read(&mut cbuf) {
            Ok(n) => {
                cbuf.truncate(n);

                let mut pos = None;
                for i in 0..n {
                    // images are newline separated
                    if cbuf[i] == b'\n' {
                        pos = Some(i);
                        break;
                    }
                }

                if let Some(pos) = pos {
                    // offset in merged buffer
                    let off = pos + buf.len() + 1;
                    buf.extend(cbuf);

                    // split into image and rest
                    let img = buf.split_to(off);
                    debug!("got image of {} bytes", img.len());
                    let _ = sender.send((topic.clone(), img.into()));
                } else {
                    buf.extend(cbuf);
                }
            }
            Err(e) => {
                error!("camera error: {e}");
            }
        }
    }
}

async fn _main() -> Result<()> {
    let config = Config::from_file(None).unwrap_or_default();
    config.setup_dirs()?;

    // set up hopper pipes for logs and images
    let mut log_pipe = Pipe::new(
        PipeMode::OUT,
        &config.ws.service_id,
        &config.channel.robot_log,
        Some(config.path.hopper.clone()),
    )?;
    let mut camera_pipe = Pipe::new(
        PipeMode::OUT,
        &config.ws.service_id,
        &config.channel.camera,
        Some(config.path.hopper.clone()),
    )?;

    log_pipe.open()?;
    camera_pipe.open()?;

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
    let (camera_sender, _) =
        watch::channel::<(String, Bytes)>((config.channel.camera.clone(), Bytes::new()));

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
        let _ = dispatch_log_messages(log_pipe, log_sender, log_topic);
    });

    // dispatch images forever
    let image_sender = camera_sender.clone();
    let image_topic = config.channel.camera.clone();
    tokio::task::spawn_blocking(move || {
        let _ = dispatch_images(camera_pipe, image_sender, image_topic);
    });

    let listener = TcpListener::bind(format!("{}:{}", &config.ws.host, config.ws.port)).await?;

    tokio::select! {
        res = async {
            loop {
                match listener.accept().await {
                    Ok((stream, _)) => {
                        // spawn a task to handle this websocket, exists for websocket lifetime
                        tokio::spawn(handle_websocket_connection(stream, WsState {
                            camera: config.channel.camera.clone(),
                            cam_rx: camera_sender.subscribe(),
                            msg_rx: msg_sender.subscribe(),
                        }));
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
