use std::collections::HashMap;

use itertools::Itertools;
use serde::{Deserialize, Serialize};
use strum::IntoEnumIterator;

use super::super::protocol::{AudioPreset, LedBrightness};
use super::super::{MAX_VOLUME_LEVEL, MIN_VOLUME_LEVEL};
use super::common::*;
use crate::bluos::SettingState;
use crate::types::DeviceId;

#[derive(Debug, Clone, Deserialize, Serialize)]
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
    /// The source selection of this device, if `None` use the current source selection
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceSelection>,
    /// Treble equalizer offset, if `None` use the current treble equalizer offset.
    /// NB: this settings is not available on all devices
    #[serde(skip_serializing_if = "Option::is_none")]
    pub treble_eq: Option<f64>,
    /// Bass equalizer offset, if `None` use the current bass equalizer offset.
    /// NB: this settings is not available on all devices
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bass_eq: Option<f64>,
    /// Center trim offset, if `None` use the current center trim offset.
    /// NB: this settings is not available on all devices
    #[serde(skip_serializing_if = "Option::is_none")]
    pub center_trim: Option<f64>,
    /// Surround upmixer, if `None` use the current surround upmixer value.
    /// NB: this settings is not available on all devices
    #[serde(skip_serializing_if = "Option::is_none")]
    pub surround_upmixer: Option<SettingState>,
    /// Virtualizer, if `None` use the current virtualizer value.
    /// NB: this settings is not available on all devices
    #[serde(skip_serializing_if = "Option::is_none")]
    pub virtualizer: Option<SettingState>,
    /// Volume leveler, if `None` use the current volume leveler value.
    /// NB: this settings is not available on all devices
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume_leveler: Option<SettingState>,
}

impl Profile for DeviceProfile {
    fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            (MIN_VOLUME_LEVEL..=MAX_VOLUME_LEVEL).contains(&self.volume_level.unwrap_or(0)),
            "invalid volume level (allowed: {MIN_VOLUME_LEVEL} - {MAX_VOLUME_LEVEL})"
        );

        if let Some(node_name) = self.node_name.as_deref() {
            validate_name(node_name)?;
        }
        if let Some(led_brightness) = self.led_brightness {
            anyhow::ensure!(
                led_brightness != LedBrightness::Unknown,
                "led brightness invalid, must be one of: {}",
                LedBrightness::iter().map(|v| v.to_string()).join(", ")
            );
        }
        if let Some(audio_preset) = self.audio_preset {
            anyhow::ensure!(
                audio_preset != AudioPreset::Unknown,
                "audio preset invalid, must be one of: {}",
                AudioPreset::iter().map(|v| v.to_string()).join(", ")
            );
        }
        if let Some(source) = self.source.as_ref() {
            source.validate()?;
        }

        Ok(())
    }

    #[tracing::instrument(err, skip_all)]
    async fn apply(self, clients: SharedClientMap) -> anyhow::Result<()> {
        let clients = clients.read().await.to_owned();
        let client = try_find_client_by_id(&clients, &self.device_id)?;
        let facts = HashMap::from([(self.device_id, DeviceFacts::gather_for_one(client).await?)]);

        if let Some(volume_level) = self.volume_level {
            client.set_volume_level(volume_level, false).await?;
        }
        if let Some(node_name) = self.node_name.as_deref() {
            client.set_node_name(node_name).await?;
        }
        if let Some(brightness) = self.led_brightness {
            client.set_led_brightness(brightness).await?;
        }
        // NB: Apply input source before audio preset
        if let Some(source) = self.source.as_ref() {
            source.apply(&clients, &facts, &self.device_id).await?;
        }
        if let Some(db) = self.treble_eq {
            client.set_equalizer_treble(db).await?;
        }
        if let Some(db) = self.bass_eq {
            client.set_equalizer_bass(db).await?;
        }
        if let Some(db) = self.center_trim {
            client.set_equalizer_center_trim(db).await?;
        }
        if let Some(state) = self.surround_upmixer {
            client.set_equalizer_upmixer(state).await?;
        }
        if let Some(state) = self.virtualizer {
            client.set_equalizer_virtualizer(state).await?;
        }
        if let Some(state) = self.volume_leveler {
            client.set_equalizer_volume_leveler(state).await?;
        }
        if let Some(audio_preset) = self.audio_preset {
            client.set_audio_preset(audio_preset).await?;
        }

        Ok(())
    }
}
