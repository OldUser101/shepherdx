use anyhow::Result;
use bytes::{Bytes, BytesMut};
use hopper::Pipe;
use shepherd_mqtt::messages::{ControlMessage, ControlMessageType};
use tokio::sync::{broadcast, watch};
use tracing::{debug, error};

use crate::buffer::LogBufferHandle;

const HOPPER_BUF_SIZE: usize = 65536;

/// forward raw mqtt messages to websockets
pub async fn dispatch_mqtt_message(
    sender: broadcast::Sender<(String, Bytes)>,
    log_handle: LogBufferHandle,
    topic: String,
    robot_control: String,
    msg: Bytes,
) -> Result<()> {
    if topic == robot_control
        && let Ok(msg) = serde_json::from_slice::<ControlMessage>(&msg)
        && msg._type == ControlMessageType::Reset
    {
        // clear logs when reset message received
        let _ = log_handle.clear();
    }

    // broadcast everywhere, result doesn't matter much
    let _ = sender.send((topic, msg));
    Ok(())
}

/// read logs from hopper and push them to websockets
pub fn dispatch_log_messages(
    sender: broadcast::Sender<(String, Bytes)>,
    log_handle: LogBufferHandle,
    log_pipe: Pipe,
    topic: String,
) -> Result<()> {
    let mut buf = BytesMut::with_capacity(HOPPER_BUF_SIZE);

    loop {
        buf.resize(HOPPER_BUF_SIZE, 0);

        match log_pipe.read(&mut buf) {
            Ok(n) => {
                buf.truncate(n);
                let _ = sender.send((topic.clone(), buf.clone().into()));
                let _ = log_handle.append(buf.clone().into());
            }
            Err(e) => {
                error!("log error: {e}");
            }
        }
    }
}

/// read images from hopper and push them to websockets
pub fn dispatch_images(
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
