use std::{fs::read_to_string, path::Path};

use anyhow::Result;
use serde::{Deserialize, Serialize};

pub const DEFAULT_CONFIG_PATH: &str = "/etc/shepherd.toml";

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub mqtt: MqttConfig,
    pub app: AppConfig,
    pub run: RunConfig,
    pub channel: ChannelConfig,
    #[serde(default = "default_robot_usb")]
    pub robot_usb: String,
    #[serde(default = "default_arena_usb")]
    pub arena_usb: String,
}

fn default_robot_usb() -> String {
    "/media/RobotUSB".to_string()
}
fn default_arena_usb() -> String {
    "/media/ArenaUSB".to_string()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            mqtt: MqttConfig::default(),
            app: AppConfig::default(),
            run: RunConfig::default(),
            channel: ChannelConfig::default(),
            robot_usb: default_robot_usb(),
            arena_usb: default_arena_usb(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MqttConfig {
    #[serde(default = "default_mqtt_broker")]
    pub broker: String,
    #[serde(default = "default_mqtt_port")]
    pub port: u16,
}

fn default_mqtt_broker() -> String {
    "localhost".to_string()
}
fn default_mqtt_port() -> u16 {
    1883
}

impl Default for MqttConfig {
    fn default() -> Self {
        Self {
            broker: default_mqtt_broker(),
            port: default_mqtt_port(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default = "default_app_service_id")]
    pub service_id: String,
    #[serde(default = "default_app_host")]
    pub host: String,
    #[serde(default = "default_app_port")]
    pub port: u16,
    #[serde(default = "default_app_static_dir")]
    pub static_dir: String,
    #[serde(default = "default_app_user_src_dir")]
    pub user_src_dir: String,
    #[serde(default = "default_app_user_cur_dir")]
    pub user_cur_dir: String,
    #[serde(default = "default_app_team_image")]
    pub team_image: String,
}

fn default_app_service_id() -> String {
    "shepherd-app".to_string()
}
fn default_app_host() -> String {
    "0.0.0.0".to_string()
}
fn default_app_port() -> u16 {
    8080
}
fn default_app_static_dir() -> String {
    "/var/shepherd/static".to_string()
}
fn default_app_user_src_dir() -> String {
    "/var/shapherd/usercode/projects".to_string()
}
fn default_app_user_cur_dir() -> String {
    "/var/shapherd/usercode/current".to_string()
}
fn default_app_team_image() -> String {
    "/var/shepherd/team-image.jpg".to_string()
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            service_id: default_app_service_id(),
            host: default_app_host(),
            port: default_app_port(),
            static_dir: default_app_static_dir(),
            user_src_dir: default_app_user_src_dir(),
            user_cur_dir: default_app_user_cur_dir(),
            team_image: default_app_team_image(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RunConfig {
    #[serde(default = "default_run_service_id")]
    pub service_id: String,
    #[serde(default = "default_run_start_button")]
    pub start_button: u32,
    #[serde(default = "default_run_gpio_device")]
    pub gpio_device: String,
}

fn default_run_service_id() -> String {
    "shepherd-run".to_string()
}
fn default_run_start_button() -> u32 {
    26
}
fn default_run_gpio_device() -> String {
    "gpiochip0".to_string()
}

impl Default for RunConfig {
    fn default() -> Self {
        Self {
            service_id: default_run_service_id(),
            start_button: default_run_start_button(),
            gpio_device: default_run_gpio_device(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ChannelConfig {
    #[serde(default = "default_channel_robot_control")]
    pub robot_control: String,
}

fn default_channel_robot_control() -> String {
    "robot/control".to_string()
}

impl Default for ChannelConfig {
    fn default() -> Self {
        Self {
            robot_control: default_channel_robot_control(),
        }
    }
}

impl Config {
    pub fn from_file(path: Option<&Path>) -> Result<Self> {
        let path = path.unwrap_or(Path::new(DEFAULT_CONFIG_PATH));
        let data = read_to_string(path)?;
        let cfg: Self = toml::from_str(&data)?;
        Ok(cfg)
    }
}
