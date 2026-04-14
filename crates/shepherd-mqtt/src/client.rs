use std::{collections::HashMap, pin::Pin, time::Duration};

use futures::future::join_all;
use rumqttc::{AsyncClient, Event, EventLoop, MqttOptions, Packet, QoS};
use tracing::{debug, warn};

use crate::messages::MqttMessage;

pub type MqttHandler =
    Box<dyn Fn(&[u8]) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send>> + Send + Sync>;

pub struct MqttClient {
    client: AsyncClient,
    event_loop: EventLoop,
    registry: HashMap<String, Vec<MqttHandler>>,
}

impl MqttClient {
    pub fn new(service_id: String, hostname: String, port: u16) -> Self {
        let mut mqttoptions = MqttOptions::new(service_id, hostname, port);
        mqttoptions.set_keep_alive(Duration::from_secs(5));

        let (client, event_loop) = AsyncClient::new(mqttoptions, 10);

        debug!("initialised new mqtt client");

        Self {
            client,
            event_loop,
            registry: HashMap::new(),
        }
    }

    pub async fn run(&mut self) -> anyhow::Result<()> {
        loop {
            match self
                .event_loop
                .poll()
                .await
                .map_err(|e| anyhow::anyhow!("event loop poll failed: {e}"))?
            {
                Event::Incoming(Packet::Publish(p)) => {
                    debug!("mqtt client got publish on '{}'", p.topic);

                    if let Some(handlers) = self.registry.get(&p.topic) {
                        let futures = handlers.iter().map(|h| h(&p.payload));
                        let results = join_all(futures).await;

                        for r in results {
                            if let Err(e) = r {
                                warn!("handler for '{}' returned error: {e}", p.topic);
                            }
                        }
                    }
                }
                Event::Incoming(Packet::Connect(c)) => {
                    debug!("mqtt client connected with id '{}'", c.client_id);
                }
                Event::Incoming(Packet::Disconnect) => {
                    debug!("mqtt client disconnected");
                    // TODO: is this a problem?
                    return Ok(());
                }
                _ => {}
            }
        }
    }

    pub async fn subscribe<T, F, Fut>(&mut self, topic: String, f: F) -> anyhow::Result<()>
    where
        T: MqttMessage,
        F: Fn(T) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        let handler: MqttHandler = Box::new(move |b| {
            let msg: anyhow::Result<T> = serde_json::from_slice(b)
                .map_err(|e| anyhow::anyhow!("failed to deserialize message : {e}"));

            match msg {
                Err(e) => Box::pin(async { Err(e) }),
                Ok(msg) => Box::pin(f(msg)),
            }
        });

        self.registry
            .entry(topic.clone())
            .or_default()
            .push(handler);
        self.client.subscribe(&topic, QoS::AtMostOnce).await?;

        debug!("client subscribed to topic '{}'", topic);

        Ok(())
    }

    pub async fn publish<T>(&mut self, topic: String, msg: T) -> anyhow::Result<()>
    where
        T: MqttMessage,
    {
        let b = serde_json::to_vec(&msg)
            .map_err(|e| anyhow::anyhow!("failed to serialize message: {e}"))?;

        self.client
            .publish(&topic, QoS::AtLeastOnce, false, b)
            .await?;

        debug!("client published to topic '{}'", topic);

        Ok(())
    }
}
