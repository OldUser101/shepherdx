use serde::{Deserialize, Serialize};

pub mod config;

/// Generate a status channel name from a service ID
pub fn status_for<S>(service_id: S) -> String
where
    S: AsRef<str>,
{
    format!("{}/status", service_id.as_ref())
}

#[derive(Debug, Default, PartialEq, Serialize, Deserialize, Copy, Clone)]
#[serde(rename_all = "lowercase")]
pub enum RunState {
    #[default]
    Init,
    Ready,
    Running,
    PostRun,
}

#[derive(Debug, Default, PartialEq, Serialize, Deserialize, Copy, Clone)]
#[serde(rename_all = "lowercase")]
pub enum Zone {
    #[default]
    Red,
    Yellow,
    Blue,
    Green,
}

#[derive(Debug, Default, PartialEq, Serialize, Deserialize, Copy, Clone)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    #[default]
    Dev,
    Comp,
}
