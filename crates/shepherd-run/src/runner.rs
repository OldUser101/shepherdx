use anyhow::Result;
use shepherd_common::config::Config;
use shepherd_mqtt::{MqttAsyncClient, MqttClient, MqttEventLoop};
use tokio_gpiod::{Bias, Chip, EdgeDetect, Input, Lines, Options};
use tracing::warn;

pub struct Runner {
    config: Config,
    mqtt_client: MqttAsyncClient,
    mqtt_event_loop: MqttEventLoop,
    gpio_chip: Option<Chip>,
    gpio_lines: Option<Lines<Input>>,
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

        let (mqtt_client, mqtt_event_loop) = MqttClient::new(
            config.run.service_id.clone(),
            config.mqtt.broker.clone(),
            config.mqtt.port,
        );

        let (gpio_chip, gpio_lines) = match Self::setup_gpio(&config).await {
            Ok((chip, line)) => (Some(chip), Some(line)),
            Err(e) => {
                warn!("gpio setup failed: {e}");
                (None, None)
            }
        };

        Self {
            config,
            mqtt_client,
            mqtt_event_loop,
            gpio_chip,
            gpio_lines,
        }
    }

    pub async fn run(&mut self) -> Result<()> {
        tokio::select!(
            res = self.mqtt_event_loop.run() => {
                warn!("mqtt client exited {:#?}", res);
                res?
            }
        );

        Ok(())
    }
}
