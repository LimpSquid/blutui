use std::time::{Duration, Instant};

use anyhow::Context;
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use strum::IntoEnumIterator;

use super::super::client::ZoneMode;
use super::common::*;
use crate::bluos::{AudioPreset, LedBrightness, MAX_VOLUME_LEVEL, MIN_VOLUME_LEVEL};
use crate::types::DeviceId;

#[derive(Default, Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum State {
    #[default]
    Check,
    UngroupSlaves,
    UngroupMasters,
    WaitForDevices,
    CheckCapabilities,
    RenameSlaves,
    Group,
    GroupWait,
    ConfigureSlaves,
    ConfigureMaster,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MultiplayerGroupProfileSlave {
    pub device_id: DeviceId,
    pub node_name: String,
    /// Volume trim in mdB
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume_trim: Option<f64>,
    /// Led brightness, if `None` use the current brightness
    #[serde(skip_serializing_if = "Option::is_none")]
    pub led_brightness: Option<LedBrightness>,
}

impl MultiplayerGroupProfileSlave {
    pub fn validate(&self) -> anyhow::Result<()> {
        validate_name(&self.node_name)?;

        if let Some(led_brightness) = self.led_brightness {
            anyhow::ensure!(
                led_brightness != LedBrightness::Unknown,
                "led brightness invalid, must be one of: {}",
                LedBrightness::iter().map(|v| v.to_string()).join(", ")
            );
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MultiplayerGroupProfile {
    pub master: DeviceId,
    /// Volume level 0 - 100, if `None` use the current level
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume_level: Option<u8>,
    /// Node name, if `None` use the current name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_name: Option<String>,
    /// Audio preset, if `None` use the current audio preset value.
    /// NB: this settings is not available on all devices
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio_preset: Option<AudioPreset>,
    /// Led brightness, if `None` use the current brightness
    #[serde(skip_serializing_if = "Option::is_none")]
    pub led_brightness: Option<LedBrightness>,
    /// The source selection of this group, if `None` use the current source selection
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceSelection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_name: Option<String>,
    pub slaves: Vec<MultiplayerGroupProfileSlave>,
    /// Extra devices to ungroup. In certain cases we cannot determine
    /// which devices need to be ungrouped to form the new group specified
    /// by this profile. For example a device that is currently part of a
    /// fixed group and acts as secondary device called a zone slave. A zone
    /// slave does not announce itself, lives in its own network formed by the
    /// master and the master does not announce the device ID of the zone slave
    /// itself. In order to make one of the zone slaves available for the profile
    /// that is about to be applied, the master of the fixed group needs to be
    /// ungrouped. Note that you do not need to specify the zone master of a fixed
    /// group. A zone master does still announce itself, if the zone master needs
    /// to be part of the group formed by this profile, it will automatically be
    /// ungrouped.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ungroup_extra: Option<Vec<DeviceId>>,
}

impl MultiplayerGroupProfile {
    fn device_ids(&self) -> impl Iterator<Item = DeviceId> {
        self.slaves
            .iter()
            .map(|s| s.device_id)
            .chain(std::iter::once(self.master))
            .chain(self.ungroup_extra.iter().flatten().copied())
    }
}

impl Profile for MultiplayerGroupProfile {
    fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            (MIN_VOLUME_LEVEL..=MAX_VOLUME_LEVEL).contains(&self.volume_level.unwrap_or(0)),
            "invalid volume level (allowed: {MIN_VOLUME_LEVEL} - {MAX_VOLUME_LEVEL})"
        );

        anyhow::ensure!(
            !self.slaves.is_empty(),
            "at least one slave needs to be specified"
        );

        self.slaves
            .iter()
            .try_for_each(MultiplayerGroupProfileSlave::validate)?;

        anyhow::ensure!(
            !self
                .slaves
                .iter()
                .map(|s| s.device_id)
                .contains(&self.master),
            "master device is also specified in slaves"
        );

        anyhow::ensure!(
            self.slaves.iter().map(|s| s.device_id).unique().count() == self.slaves.len(),
            "duplicate slave specified"
        );

        anyhow::ensure!(
            self.slaves
                .iter()
                .map(|s| s.node_name.as_str())
                .unique()
                .count()
                == self.slaves.len(),
            "duplicate slave node name specified"
        );

        if let Some(node_name) = self.node_name.as_deref() {
            validate_name(node_name)?;
        }
        if let Some(group_name) = self.group_name.as_deref() {
            validate_name(group_name)?;
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
        self.run(clients).await
    }
}

impl StateMachine for MultiplayerGroupProfile {
    type State = State;

    async fn run_state(
        &self,
        state: &Self::State,
        facts: &FactMap,
        clients: &ClientMap,
        transition_time_point: Instant,
    ) -> anyhow::Result<NextState<Self::State>> {
        match state {
            State::Check => match try_find_facts_by_id(facts, &self.master) {
                Ok(facts)
                    if facts.group_status.slave.is_empty()
                        && facts.group_status.zone_slave.len() == self.slaves.len()
                        && self.slaves.iter().all(|s| {
                            facts
                                .group_status
                                .zone_slave
                                .iter()
                                .find(|zs| {
                                    zs.name.as_deref().is_some_and(|name| name == s.node_name)
                                })
                                .is_some()
                        }) =>
                {
                    Ok(NextState::Immediate(State::ConfigureSlaves))
                }
                _ => Ok(NextState::Immediate(State::UngroupSlaves)),
            },
            State::UngroupSlaves => {
                ungroup_slaves_from_master(self.device_ids(), facts, clients).await?;

                Ok(NextState::After(
                    Duration::from_secs(1),
                    State::UngroupMasters,
                ))
            }
            State::UngroupMasters => {
                ungroup_masters(self.device_ids(), facts, clients).await?;

                Ok(NextState::Immediate(State::WaitForDevices))
            }
            State::WaitForDevices => {
                let not_found: Vec<_> = self
                    .slaves
                    .iter()
                    .map(|s| &s.device_id)
                    .chain(std::iter::once(&self.master))
                    // Wait until device is reachable
                    .filter(|device_id| try_find_facts_by_id(facts, device_id).is_err())
                    .collect();

                if not_found.is_empty() {
                    Ok(NextState::Immediate(State::CheckCapabilities))
                } else {
                    anyhow::ensure!(
                        transition_time_point.elapsed() < Duration::from_secs(90),
                        "timeout waiting on devices to become available, not found: {}",
                        not_found.into_iter().join(", ")
                    );

                    Ok(NextState::After(
                        Duration::from_secs(5),
                        State::WaitForDevices,
                    ))
                }
            }
            State::CheckCapabilities => {
                anyhow::ensure!(
                    try_find_facts_by_id(facts, &self.master)?
                        .group_status
                        .zone_options
                        .as_ref()
                        .is_some_and(|o| o.option.iter().any(|o| o.channel.can_be_master())),
                    "master '{}' does not support multiplayer grouping",
                    self.master
                );

                for slave in self.slaves.iter().map(|s| &s.device_id) {
                    anyhow::ensure!(
                        try_find_facts_by_id(facts, slave)?
                            .group_status
                            .zone_options
                            .as_ref()
                            .is_some_and(|o| o.option.iter().any(|o| o.channel.can_be_slave())),
                        "slave '{slave}' does not support multiplayer grouping"
                    );
                }

                Ok(NextState::Immediate(State::RenameSlaves))
            }
            State::RenameSlaves => {
                for slave in &self.slaves {
                    try_find_client_by_id(clients, &slave.device_id)?
                        .set_node_name(&slave.node_name)
                        .await?;
                }

                Ok(NextState::Immediate(State::Group))
            }
            State::Group => {
                let endpoints_to_add: Vec<_> = self
                    .slaves
                    .iter()
                    .map(|s| &s.device_id)
                    .filter_map(|s| try_find_client_by_id(clients, s).ok())
                    .map(|client| client.ip_and_port())
                    .collect();

                anyhow::ensure!(!endpoints_to_add.is_empty(), "no devices found to group");

                try_find_client_by_id(clients, &self.master)?
                    .add_slaves(
                        &endpoints_to_add,
                        ZoneMode::MultiplayerGroup {
                            group_name: self.group_name.clone(),
                        },
                    )
                    .await?;

                Ok(NextState::Immediate(State::GroupWait))
            }
            State::GroupWait => {
                let facts = try_find_facts_by_id(facts, &self.master)?;

                if self.slaves.iter().all(|s| {
                    facts
                        .group_status
                        .zone_slave
                        .iter()
                        .find(|zs| zs.name.as_deref().is_some_and(|name| name == s.node_name))
                        .is_some()
                }) {
                    Ok(NextState::Immediate(State::ConfigureSlaves))
                } else {
                    anyhow::ensure!(
                        transition_time_point.elapsed() < Duration::from_secs(30),
                        "timeout waiting on zone slaves to become available"
                    );

                    Ok(NextState::After(Duration::from_secs(5), State::GroupWait))
                }
            }
            State::ConfigureSlaves => {
                let client = try_find_client_by_id(clients, &self.master)?;
                let facts = try_find_facts_by_id(facts, &self.master)?;

                for profile in self.slaves.iter() {
                    let zone_slave = facts
                        .group_status
                        .zone_slave
                        .iter()
                        .find(|zs| {
                            zs.name
                                .as_deref()
                                .is_some_and(|name| name == profile.node_name)
                        })
                        .context(format!(
                            "zone slave not found in group: {}",
                            profile.node_name
                        ))?;

                    if let Some(volume_trim) = profile.volume_trim {
                        client
                            .set_zone_slave_volume_level(
                                (zone_slave.ip_addr, *zone_slave.port),
                                volume_trim,
                            )
                            .await?;
                    }
                    if let Some(brightness) = profile.led_brightness {
                        client
                            .set_zone_slave_led_brightness(
                                (zone_slave.ip_addr, *zone_slave.port),
                                brightness,
                            )
                            .await?;
                    }
                }

                Ok(NextState::Immediate(State::ConfigureMaster))
            }
            State::ConfigureMaster => {
                let client = try_find_client_by_id(clients, &self.master)?;

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
                    source.apply(clients, facts, &self.master).await?;
                }
                if let Some(audio_preset) = self.audio_preset {
                    client.set_audio_preset(audio_preset).await?;
                }

                Ok(NextState::Finished)
            }
        }
    }
}
