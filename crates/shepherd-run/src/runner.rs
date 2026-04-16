use anyhow::Result;
use shepherd_mqtt::{MqttAsyncClient, MqttClient, MqttEventLoop};
use tokio_gpiod::{Bias, Chip, Input, Lines, Options};
use tracing::warn;

const SERVICE_ID: &str = "shepherd-run";
const GPIO_CHIP: &str = "gpiochip0";
const START_BUTTON_PIN: u32 = 26;

pub struct Runner {
    mqtt_client: MqttAsyncClient,
    mqtt_event_loop: MqttEventLoop,
    gpio_chip: Option<Chip>,
    gpio_lines: Option<Lines<Input>>,
}

impl Runner {
    async fn setup_gpio() -> Result<(Chip, Lines<Input>)> {
        let chip = Chip::new(GPIO_CHIP).await?;
        let opts = Options::input([START_BUTTON_PIN])
            .edge(tokio_gpiod::EdgeDetect::Falling)
            .bias(Bias::PullUp)
            .consumer(SERVICE_ID);
        let lines = chip.request_lines(opts).await?;
        Ok((chip, lines))
    }

    pub async fn new() -> Self {
        let (mqtt_client, mqtt_event_loop) =
            MqttClient::new(SERVICE_ID.to_string(), "localhost".to_string(), 1883);

        let (gpio_chip, gpio_lines) = match Self::setup_gpio().await {
            Ok((chip, line)) => (Some(chip), Some(line)),
            Err(e) => {
                warn!("gpio setup failed: {e}");
                (None, None)
            }
        };

        Self {
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
