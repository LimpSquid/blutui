use anyhow::Context;
use serde::{Deserialize, Serialize};

use super::super::protocol::{AudioPreset, LedBrightness};
use super::super::{MAX_VOLUME_LEVEL, MIN_VOLUME_LEVEL};
use super::common::*;
use crate::types::DeviceId;

#[derive(Debug, Clone, Hash, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub struct DeviceProfile {
    pub device_id: DeviceId,
    /// Volume level 0 - 100, if `None` use the current level
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume_level: Option<u8>,
    /// Led brightness, if `None` use the current brightness
    #[serde(skip_serializing_if = "Option::is_none")]
    pub led_brightness: Option<LedBrightness>,
    /// Node name, if `None` use the current name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_name: Option<String>,
    /// Audio preset, if `None` use the current audio preset value.
    /// NB: this settings is not available on all devices
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio_preset: Option<AudioPreset>,
}

impl DeviceProfile {
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            (MIN_VOLUME_LEVEL..=MAX_VOLUME_LEVEL).contains(&self.volume_level.unwrap_or(0)),
            "invalid volume level (allowed: {MIN_VOLUME_LEVEL} - {MAX_VOLUME_LEVEL})"
        );

        if let Some(node_name) = self.node_name.as_deref() {
            anyhow::ensure!(node_name.len() > 0, "node name must be atleast one char");
            anyhow::ensure!(
                node_name.len() <= 32,
                "node name must be 32 chars at maximum"
            );
            anyhow::ensure!(
                node_name.is_ascii(),
                "node name must only contain ASCII chars"
            );
        }

        Ok(())
    }

    #[tracing::instrument(err, skip_all)]
    pub(super) async fn apply(self, clients: SharedClientMap) -> anyhow::Result<()> {
        self.validate().context("device profile invalid")?;

        let clients = clients.read().await.to_owned();
        let client = try_find_client_by_id(&clients, &self.device_id)?;

        if let Some(brightness) = self.led_brightness {
            client.set_led_brightness(brightness).await?;
        }
        if let Some(node_name) = self.node_name.as_deref() {
            client.set_node_name(node_name).await?;
        }
        if let Some(level) = self.volume_level {
            client.set_volume_level(level, false).await?;
        }
        if let Some(audio_preset) = self.audio_preset.as_ref() {
            client.set_audio_preset(*audio_preset).await?;
        }

        Ok(())
    }
}
