use std::{path::PathBuf, time::Duration};

use anyhow::{Result, anyhow};
use shepherd_common::{Mode, RunState, Zone, config::Config, status_for};
use shepherd_mqtt::{
    MqttAsyncClient, MqttClient,
    messages::{ControlMessage, ControlMessageType, RunStatusMessage},
};
use tokio::{
    fs,
    process::Command,
    sync::mpsc::{self, UnboundedReceiver, UnboundedSender},
};
use tokio_gpiod::{Bias, Chip, EdgeDetect, Input, Lines, Options};
use tracing::{error, info, warn};

use crate::usercode::{Usercode, UsercodeHandle};

pub enum StateEvent {
    Transition(RunState, Option<RunState>),
    SetTarget(Mode, Zone),
}

pub struct Runner {
    config: Config,
    state: RunState,
    target_mode: Mode,
    target_zone: Zone,
    usercode_handle: Option<UsercodeHandle>,
    state_sender: Option<UnboundedSender<StateEvent>>,
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

    pub async fn new(config: Config) -> Result<Self> {
        Ok(Self {
            config,
            state: RunState::Init,
            target_mode: Mode::Dev,
            target_zone: Zone::from_id(0),
            usercode_handle: None,
            state_sender: None,
        })
    }

    async fn reset_state(&mut self) {
        self.target_mode = Mode::Dev;
        self.target_zone = Zone::from_id(0);

        // spawn hardware reset script
        match Command::new(&self.config.run.reset_script).spawn() {
            Ok(mut child) => match child.wait().await {
                Ok(status) if !status.success() => {
                    warn!("reset script exited with status {:?}", status)
                }
                Err(e) => warn!("failed to wait on reset script: {e}"),
                Ok(_) => {}
            },
            Err(e) => {
                error!("failed to run robot reset script: {e}");
            }
        }
    }

    /// Copy start image to temporary location for websockets
    async fn load_start_image(&self) -> Result<()> {
        let start_image = if self.config.path.team_image.is_file() {
            &self.config.path.team_image
        } else if self.config.path.game_image.is_file() {
            &self.config.path.game_image
        } else {
            warn!("no start image found, not creating tmp graphic!");
            return Ok(());
        };

        fs::copy(start_image, self.config.path.tmp_root.join("image.jpg")).await?;

        Ok(())
    }

    /// reset usercode
    async fn state_ready(&mut self) -> Result<()> {
        if let Some(uh) = &self.usercode_handle {
            uh.start()?;
        } else {
            return Err(anyhow!("tried to start usercode, but handle was not set?"));
        }

        Ok(())
    }

    /// running transition, send start info and timeout to usercode
    async fn state_running(&mut self) -> Result<()> {
        if let Some(uh) = &self.usercode_handle {
            uh.send_start_info(self.target_mode, self.target_zone)?;

            // set round timeout only if in comp mode
            if self.target_mode == Mode::Comp {
                uh.set_timeout(Duration::from_secs(self.config.run.comp_timeout))?;
            }
        } else {
            return Err(anyhow!(
                "tried to configure usercode, but handle was not set?"
            ));
        }

        Ok(())
    }

    /// post-run transition, kill usercode, reset state
    async fn state_post_run(&mut self) -> Result<()> {
        if let Some(uh) = &self.usercode_handle {
            uh.kill()?;
        } else {
            return Err(anyhow!("tried to kill usercode, but handle was not set?"));
        }

        self.reset_state().await;
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
                    if let Some(prev) = prev
                        && self.state != prev
                    {
                        warn!(
                            "cannot switch to {:?} from {:?}, requires {:?}",
                            next, self.state, prev
                        );
                        continue;
                    }

                    info!("transition to {:?}", next);

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
                        RunState::Init => {
                            // Init only ever runs once
                            warn!("cannot transition to Init state");
                            continue;
                        }
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
        info!("got mqtt control message: {:?}", msg);
        match msg._type {
            ControlMessageType::Start => {
                sender.send(StateEvent::SetTarget(msg.mode, msg.zone))?;
                sender.send(StateEvent::Transition(
                    RunState::Running,
                    Some(RunState::Ready),
                ))?
            }
            ControlMessageType::Stop => {
                sender.send(StateEvent::Transition(RunState::PostRun, None))?
            }
            ControlMessageType::Reset => {
                sender.send(StateEvent::Transition(RunState::Ready, None))?
            }
        }
        Ok(())
    }

    /// Dispatch GPIO events to state transitions
    async fn dispatch_gpio(
        config: Config,
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

                let arena_usb = PathBuf::from(&config.path.arena_usb);

                // pull zone info from arena usb
                let zone = if arena_usb.join("zone1.txt").is_file() {
                    Zone::from_id(1)
                } else if arena_usb.join("zone2.txt").is_file() {
                    Zone::from_id(2)
                } else if arena_usb.join("zone3.txt").is_file() {
                    Zone::from_id(3)
                } else {
                    // default to zone 0 always
                    Zone::from_id(0)
                };

                sender.send(StateEvent::SetTarget(Mode::Comp, zone))?;
                sender.send(StateEvent::Transition(
                    RunState::Running,
                    Some(RunState::Ready),
                ))?;
            }
        }
    }

    /// Final setup & event dispatch loops
    pub async fn run(&mut self) -> Result<()> {
        // destroy previous sessions, if any
        self.state = RunState::Init;
        self.state_sender = None;
        self.reset_state().await;

        let (state_sender, state_receiver) = mpsc::unbounded_channel();

        let (mut mqtt_client, mut mqtt_event_loop) = MqttClient::new(
            &self.config.run.service_id,
            &self.config.mqtt.broker,
            self.config.mqtt.port,
        );

        // mqtt needs to run independently
        let mqtt_loop = tokio::spawn(async move { mqtt_event_loop.run().await });

        // transition immediately into ready when dispatch starts later
        state_sender.send(StateEvent::Transition(
            RunState::Ready,
            Some(RunState::Init),
        ))?;

        // setup mqtt receiver for control events
        let mqtt_state_sender = state_sender.clone();
        mqtt_client
            .subscribe(
                &self.config.channel.robot_control,
                move |_, msg: ControlMessage| {
                    // this feels like a hack
                    let state_sender = mqtt_state_sender.clone();
                    async move { Self::dispatch_mqtt(msg, state_sender).await }
                },
            )
            .await?;

        // setup gpio handling if available
        let gpio_config = self.config.clone();
        let gpio_state_sender = state_sender.clone();
        match Self::setup_gpio(&self.config).await {
            // we don't care about joining later, task runs anyway
            Ok((_, lines)) => std::mem::drop(tokio::task::spawn(async move {
                if let Err(e) =
                    Self::dispatch_gpio(gpio_config, lines, gpio_state_sender.clone()).await
                {
                    warn!("gpio exited with error: {e}");
                }
            })),
            Err(e) => warn!("gpio setup failed: {e}"),
        }

        // copy start image to tmp location
        self.load_start_image().await?;

        // initialise usercode manager
        let (mut usercode, usercode_handle) = Usercode::new(self.config.clone())?;

        let usercode_state_sender = state_sender.clone();
        usercode.on_exit(Some(move || {
            // force transition to post-run since usercode state is unknown
            let _ = usercode_state_sender.send(StateEvent::Transition(RunState::PostRun, None));
        }));

        self.usercode_handle = Some(usercode_handle);
        self.state_sender = Some(state_sender);

        // will abort when either of these fail
        tokio::select!(
            res = self.dispatch_state(&mqtt_client, state_receiver) => {
                warn!("state dispatch exited {:?}", res);
                res?
            }
            res = mqtt_loop => {
                warn!("mqtt client exited {:?}", res);

                match res {
                    Ok(e) => e?,
                    Err(e) => return Err(e.into()),
                }
            }
            res = usercode.run() => {
                warn!("usercode runner exited {:?}", res);
                res?
            }
        );

        Ok(())
    }
}
