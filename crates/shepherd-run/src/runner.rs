use anyhow::Result;
use shepherd_common::{Mode, RunState, Zone, config::Config, status_for};
use shepherd_mqtt::{
    MqttAsyncClient, MqttClient,
    messages::{ControlMessage, ControlMessageType, RunStatusMessage},
};
use tokio::sync::mpsc::{self, UnboundedReceiver};
use tokio_gpiod::{Bias, Chip, EdgeDetect, Input, Lines, Options};
use tracing::{info, warn};

pub enum StateEvent {
    Transition(RunState, RunState),
    SetTarget(Mode, Zone),
}

pub struct Runner {
    config: Config,
    gpio_chip: Option<Chip>,
    gpio_lines: Option<Lines<Input>>,
    state: RunState,

    target_mode: Mode,
    target_zone: Zone,
}

impl Runner {
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
        let config = Config::from_file(None).unwrap_or_default();

        let (gpio_chip, gpio_lines) = match Self::setup_gpio(&config).await {
            Ok((chip, line)) => (Some(chip), Some(line)),
            Err(e) => {
                warn!("gpio setup failed: {e}");
                (None, None)
            }
        };

        Self {
            config,
            gpio_chip,
            gpio_lines,
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

    async fn dispatch_state(
        &mut self,
        mqttc: &MqttAsyncClient,
        recv: UnboundedReceiver<StateEvent>,
    ) -> Result<()> {
        let mut recv = recv;

        while let Some(ev) = recv.recv().await {
            match ev {
                StateEvent::Transition(next, prev) => {
                    if self.state != prev {
                        warn!(
                            "cannot switch to {:#?} from {:#?}, requires {:#?}",
                            next, self.state, prev
                        );
                        continue;
                    }

                    info!("transition to {:#?}", next);

                    self.state = next;

                    mqttc
                        .publish(
                            status_for(&self.config.run.service_id),
                            RunStatusMessage { state: next },
                        )
                        .await?;

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

    pub async fn run(&mut self) -> Result<()> {
        let (state_sender, state_receiver) = mpsc::unbounded_channel();

        let (mut mqtt_client, mut mqtt_event_loop) = MqttClient::new(
            &self.config.run.service_id,
            &self.config.mqtt.broker,
            self.config.mqtt.port,
        );

        mqtt_client
            .subscribe(
                &self.config.channel.robot_control,
                move |msg: ControlMessage| {
                    let state_sender = state_sender.clone();
                    async move {
                        match msg._type {
                            ControlMessageType::Start => {
                                state_sender.send(StateEvent::SetTarget(msg.mode, msg.zone))?;
                                state_sender.send(StateEvent::Transition(
                                    RunState::Running,
                                    RunState::Ready,
                                ))?
                            }
                            ControlMessageType::Stop => state_sender.send(
                                StateEvent::Transition(RunState::PostRun, RunState::Running),
                            )?,
                        }
                        info!("got control message: {:#?}", msg);
                        Ok(())
                    }
                },
            )
            .await?;

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
