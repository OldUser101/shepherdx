use anyhow::Result;
use shepherd_common::{Mode, RunState, Zone, config::Config, status_for};
use shepherd_mqtt::{
    MqttAsyncClient, MqttClient,
    messages::{ControlMessage, ControlMessageType, RunStatusMessage},
};
use std::{path::PathBuf, sync::Arc, time::Duration};
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio_gpiod::{Bias, Chip, EdgeDetect, Input, Lines, Options};
use tracing::{info, warn};

pub enum StateEvent {
    Transition(RunState, RunState),
    SetTarget(Mode, Zone),
}

pub struct Runner {
    config: Arc<Config>,
    state: RunState,
    target_mode: Mode,
    target_zone: Zone,
}

impl Runner {
    /// Setup GPIO events for start button
    async fn setup_gpio(config: &Config) -> Result<(Chip, Lines<Input>)> {
        let chip = Chip::new(config.run.gpio_device.clone()).await?;
        let opts = Options::input([config.run.start_button])
            .edge(EdgeDetect::Falling)
            .bias(Bias::PullUp)
            .consumer(config.run.service_id.clone());
        let lines = chip.request_lines(opts).await?;
        Ok((chip, lines))
    }

    pub async fn new() -> Self {
        let config = Arc::new(Config::from_file(None).unwrap_or_default());

        Self {
            config,
            state: RunState::Init,
            target_mode: Mode::default(),
            target_zone: Zone::default(),
        }
    }

    async fn state_init(&mut self) -> Result<()> {
        Ok(())
    }

    async fn state_ready(&mut self) -> Result<()> {
        Ok(())
    }

    async fn state_running(&mut self) -> Result<()> {
        Ok(())
    }

    async fn state_post_run(&mut self) -> Result<()> {
        Ok(())
    }

    /// Dispatch incoming state transitions
    async fn dispatch_state(
        &mut self,
        mqttc: &MqttAsyncClient,
        recv: UnboundedReceiver<StateEvent>,
    ) -> Result<()> {
        let mut recv = recv;

        while let Some(ev) = recv.recv().await {
            match ev {
                StateEvent::Transition(next, prev) => {
                    // states must transition in specified sequence
                    if self.state != prev {
                        warn!(
                            "cannot switch to {:#?} from {:#?}, requires {:#?}",
                            next, self.state, prev
                        );
                        continue;
                    }

                    info!("transition to {:#?}", next);

                    self.state = next;

                    // publish a status message for consumers
                    // could be used to tell when robot is started/stopped
                    mqttc
                        .publish(
                            status_for(&self.config.run.service_id),
                            RunStatusMessage { state: next },
                        )
                        .await?;

                    // call handler functions for state transitions
                    match next {
                        RunState::Init => self.state_init().await,
                        RunState::Ready => self.state_ready().await,
                        RunState::Running => self.state_running().await,
                        RunState::PostRun => self.state_post_run().await,
                    }?;
                }
                StateEvent::SetTarget(mode, zone) => {
                    self.target_mode = mode;
                    self.target_zone = zone;

                    info!("update (mode, zone) to ({:#?}, {:#?})", mode, zone);
                }
            }
        }

        Ok(())
    }

    /// Dispatch MQTT events to state transitions
    async fn dispatch_mqtt(msg: ControlMessage, sender: UnboundedSender<StateEvent>) -> Result<()> {
        info!("got control message: {:#?}", msg);
        match msg._type {
            ControlMessageType::Start => {
                sender.send(StateEvent::SetTarget(msg.mode, msg.zone))?;
                sender.send(StateEvent::Transition(RunState::Running, RunState::Ready))?
            }
            ControlMessageType::Stop => {
                sender.send(StateEvent::Transition(RunState::PostRun, RunState::Running))?
            }
        }
        Ok(())
    }

    /// Dispatch GPIO events to state transitions
    async fn dispatch_gpio(
        config: Arc<Config>,
        lines: Lines<Input>,
        sender: UnboundedSender<StateEvent>,
    ) -> Result<()> {
        let mut lines = lines;
        let mut last_event = Duration::ZERO;
        loop {
            let event = lines.read_event().await?;
            if event.time - last_event >= Duration::from_millis(1000) {
                info!("gpio start detected");
                last_event = event.time;

                let arena_usb = PathBuf::from(&config.arena_usb);

                // pull zone info from arena usb
                let zone = if arena_usb.join("zone1.txt").is_file() {
                    Zone::from_id(1)
                } else if arena_usb.join("zone2.txt").is_file() {
                    Zone::from_id(2)
                } else if arena_usb.join("zone3.txt").is_file() {
                    Zone::from_id(3)
                } else {
                    Zone::from_id(0)
                };

                sender.send(StateEvent::SetTarget(Mode::Comp, zone))?;
                sender.send(StateEvent::Transition(RunState::Running, RunState::Ready))?;
            }
        }
    }

    /// Final setup & event dispatch loops
    pub async fn run(&mut self) -> Result<()> {
        let (state_sender, state_receiver) = mpsc::unbounded_channel();

        let (mut mqtt_client, mut mqtt_event_loop) = MqttClient::new(
            &self.config.run.service_id,
            &self.config.mqtt.broker,
            self.config.mqtt.port,
        );

        // setup mqtt receiver for control events
        let mqtt_state_sender = state_sender.clone();
        mqtt_client
            .subscribe(
                &self.config.channel.robot_control,
                move |msg: ControlMessage| {
                    // this feels like a hack
                    let state_sender = mqtt_state_sender.clone();
                    async move { Self::dispatch_mqtt(msg, state_sender).await }
                },
            )
            .await?;

        // setup gpio handling if available
        let gpio_config = self.config.clone();
        match Self::setup_gpio(&self.config).await {
            // we don't care about joining later, task runs anyway
            Ok((_, lines)) => std::mem::drop(tokio::task::spawn(async move {
                Self::dispatch_gpio(gpio_config, lines, state_sender.clone()).await
            })),
            Err(e) => warn!("gpio setup failed: {e}"),
        }

        // will abort when either of these fail
        tokio::select!(
            res = self.dispatch_state(&mqtt_client, state_receiver) => {
                warn!("state dispatch exited {:#?}", res);
                res?
            }
            res = mqtt_event_loop.run() => {
                warn!("mqtt client exited {:#?}", res);
                res?
            }
        );

        Ok(())
    }
}
