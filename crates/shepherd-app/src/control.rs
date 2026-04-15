use axum::{Json, Router, extract::State, http::StatusCode, routing::post};
use serde::{Deserialize, Serialize};
use shepherd_common::{Mode, Zone};
use shepherd_mqtt::{
    MqttAsyncClient,
    messages::{ControlMessage, ControlMessageType},
};

use crate::error::{ShepherdError, ShepherdResult};

#[derive(Clone)]
struct ControlState {
    mqttc: MqttAsyncClient,
}

#[derive(Debug, Serialize, Deserialize)]
struct ControlRequest {
    zone: Zone,
    mode: Mode,
}

async fn start(
    State(mut state): State<ControlState>,
    Json(payload): Json<ControlRequest>,
) -> ShepherdResult<()> {
    let msg = ControlMessage {
        _type: ControlMessageType::Start,
        zone: payload.zone,
        mode: payload.mode,
    };

    state
        .mqttc
        .publish("robot/control".to_string(), msg)
        .await
        .map_err(|e| {
            ShepherdError(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("mqtt error: {e}"),
            )
        })?;

    Ok(())
}

async fn stop(State(mut state): State<ControlState>) -> ShepherdResult<()> {
    let msg = ControlMessage {
        _type: ControlMessageType::Stop,
        zone: Zone::default(),
        mode: Mode::default(),
    };

    state
        .mqttc
        .publish("robot/control".to_string(), msg)
        .await
        .map_err(|e| {
            ShepherdError(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("mqtt error: {e}"),
            )
        })?;

    Ok(())
}

pub fn router(mqttc: MqttAsyncClient) -> Router {
    Router::new()
        .route("/start", post(start))
        .route("/stop", post(stop))
        .with_state(ControlState { mqttc })
}
