use anyhow::Context;
use serde_derive::Deserialize;
use std::{collections::HashMap, net::SocketAddr};

pub struct SerialPortSettings {
    pub path: String,
    pub baud_rate: u32,
}

pub type ServerConfig = HashMap<SocketAddr, SerialPortSettings>;

#[derive(Deserialize)]
struct DirtySerialPortSettings {
    path: Option<String>,
    baud_rate: Option<u32>,
}

impl DirtySerialPortSettings {
    fn clean(
        self,
        defaults: Option<&DirtySerialPortSettings>,
    ) -> anyhow::Result<SerialPortSettings> {
        let result = SerialPortSettings {
            path: self
                .path
                .or_else(|| defaults.and_then(|d| d.path.clone()))
                .ok_or_else(|| {
                    anyhow::anyhow!("'path' field is missing and there is no default setting")
                })?,
            baud_rate: self
                .baud_rate
                .or_else(|| defaults.and_then(|d| d.baud_rate))
                .ok_or_else(|| {
                    anyhow::anyhow!("'baud_rate' field is missing and there is no default setting")
                })?,
        };

        Ok(result)
    }
}

#[derive(Deserialize)]
pub struct DirtyServerConfig {
    default_settings: Option<DirtySerialPortSettings>,
    #[serde(flatten)]
    mappings: HashMap<SocketAddr, DirtySerialPortSettings>,
}

impl TryFrom<DirtyServerConfig> for ServerConfig {
    type Error = anyhow::Error;

    fn try_from(dirty: DirtyServerConfig) -> Result<Self, Self::Error> {
        dirty
            .mappings
            .into_iter()
            .map(|(socket, dirty_settings)| {
                let clean_settings = dirty_settings
                    .clean(dirty.default_settings.as_ref())
                    .with_context(|| {
                        format!("invalid serial port settings for TCP port {}", socket)
                    })?;
                Ok((socket, clean_settings))
            })
            .collect::<anyhow::Result<ServerConfig>>()
    }
}
