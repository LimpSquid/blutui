use std::collections::HashMap;
use std::fmt::Debug;
use std::time::{Duration, Instant};

use anyhow::Context;
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use strum::IntoEnumIterator;
use tokio::time::sleep;

use super::super::client::ZoneMode;
use super::common::*;
use crate::bluos::{AudioPreset, LedBrightness};
use crate::types::DeviceId;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum State {
    Wait {
        duration: Duration,
        next_state: Box<Self>,
    },

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
    Finished,
}

impl State {
    fn should_gather_facts(&self) -> bool {
        match self {
            Self::Check
            | Self::UngroupSlaves
            | Self::UngroupMasters
            | Self::WaitForDevices
            | Self::CheckCapabilities
            | Self::RenameSlaves
            | Self::Group
            | Self::GroupWait
            | Self::ConfigureSlaves
            | Self::ConfigureMaster => true,
            Self::Wait { .. } | Self::Finished => false,
        }
    }
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
        // Ensure valid node name
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
    /// Audio preset, if `None` use the current audio preset value.
    /// NB: this settings is not available on all devices
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio_preset: Option<AudioPreset>,
    /// Led brightness, if `None` use the current brightness
    #[serde(skip_serializing_if = "Option::is_none")]
    pub led_brightness: Option<LedBrightness>,
    pub slaves: Vec<MultiplayerGroupProfileSlave>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_name: Option<String>,
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
    pub fn validate(&self) -> anyhow::Result<()> {
        // Must be one slave in the group
        anyhow::ensure!(
            !self.slaves.is_empty(),
            "at least one slave needs to be specified"
        );

        // Validate slaves
        self.slaves
            .iter()
            .try_for_each(MultiplayerGroupProfileSlave::validate)?;

        // Master cannot be specified in slaves
        anyhow::ensure!(
            !self
                .slaves
                .iter()
                .map(|s| s.device_id)
                .contains(&self.master),
            "master device is also specified in slaves"
        );

        // There can be no duplicate slaves
        anyhow::ensure!(
            self.slaves.iter().map(|s| s.device_id).unique().count() == self.slaves.len(),
            "duplicate slave specified"
        );

        // There can be no duplicate node names
        anyhow::ensure!(
            self.slaves
                .iter()
                .map(|s| s.node_name.as_str())
                .unique()
                .count()
                == self.slaves.len(),
            "duplicate slave node name specified"
        );

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

        Ok(())
    }

    #[tracing::instrument(err, skip_all)]
    pub(super) async fn apply(self, clients: SharedClientMap) -> anyhow::Result<()> {
        self.validate()
            .context("multiplayer group profile invalid")?;
        let mut statemachine = State::Check;
        let mut transition_time_point = Instant::now();

        loop {
            let clients = clients.read().await.to_owned();
            let facts = if statemachine.should_gather_facts() {
                DeviceFacts::gather_for_all(clients.clone()).await?
            } else {
                Default::default()
            };

            tracing::debug!(?statemachine, ?facts, "executing step");

            let next_state = match &statemachine {
                State::Wait {
                    duration,
                    next_state,
                } => {
                    anyhow::ensure!(
                        !matches!(**next_state, State::Wait { .. }),
                        "illegal next state"
                    );
                    sleep(*duration).await;
                    next_state.as_ref().to_owned()
                }

                State::Check => match try_find_facts_by_id(&facts, &self.master) {
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
                        State::ConfigureSlaves
                    }
                    _ => State::UngroupSlaves,
                },
                State::UngroupSlaves => {
                    for ((master_ip, master_port), endpoints_to_remove) in self
                        .slaves
                        .iter()
                        .map(|s| &s.device_id)
                        .chain(std::iter::once(&self.master))
                        .chain(self.ungroup_extra.clone().unwrap_or_default().iter())
                        .filter_map(|device_id| try_find_facts_by_id(&facts, &device_id).ok())
                        // NB: should not be needed, but makes the context clear
                        .filter(|s| s.group_status.am_i_slave())
                        .filter_map(|s| {
                            let m = s.group_status.master.as_ref()?;
                            Some(((m.ip_addr, m.port), s.group_status.id))
                        })
                        .fold(
                            HashMap::<_, Vec<_>>::new(),
                            |mut acc, (master_of_slave, slave)| {
                                acc.entry(master_of_slave).or_default().push(slave);
                                acc
                            },
                        )
                    {
                        // Remove slaves from the master node
                        try_find_client_by_ip_and_port(&clients, master_ip, master_port)?
                            .remove_slaves(&endpoints_to_remove)
                            .await?;
                    }

                    State::Wait {
                        duration: Duration::from_secs(1),
                        next_state: Box::new(State::UngroupMasters),
                    }
                }
                State::UngroupMasters => {
                    for ((master_ip, master_port), endpoints_to_remove) in self
                        .slaves
                        .iter()
                        .map(|s| &s.device_id)
                        .chain(std::iter::once(&self.master))
                        .chain(self.ungroup_extra.clone().unwrap_or_default().iter())
                        .filter_map(|device_id| try_find_facts_by_id(&facts, &device_id).ok())
                        .filter(|s| s.group_status.am_i_master())
                        .map(|s| {
                            (
                                s.group_status.id,
                                s.group_status
                                    .slave
                                    .iter()
                                    .map(|s| (s.ip_addr, *s.port))
                                    .chain(
                                        s.group_status
                                            .zone_slave
                                            .iter()
                                            .map(|s| (s.ip_addr, *s.port)),
                                    )
                                    .collect::<Vec<_>>(),
                            )
                        })
                    {
                        // Remove all slaves from the master node
                        try_find_client_by_ip_and_port(&clients, master_ip, master_port)?
                            .remove_slaves(&endpoints_to_remove)
                            .await?;
                    }

                    State::WaitForDevices
                }
                State::WaitForDevices => {
                    let not_found: Vec<_> = self
                        .slaves
                        .iter()
                        .map(|s| &s.device_id)
                        .chain(std::iter::once(&self.master))
                        // Wait until device is reachable
                        .filter(|device_id| try_find_facts_by_id(&facts, device_id).is_err())
                        .collect();

                    if not_found.is_empty() {
                        State::CheckCapabilities
                    } else {
                        anyhow::ensure!(
                            transition_time_point.elapsed() < Duration::from_secs(90),
                            "timeout waiting on devices to become available, not found: {}",
                            not_found.into_iter().join(", ")
                        );

                        sleep(Duration::from_secs(1)).await;
                        State::WaitForDevices
                    }
                }
                State::CheckCapabilities => {
                    anyhow::ensure!(
                        try_find_facts_by_id(&facts, &self.master)?
                            .group_status
                            .zone_options
                            .as_ref()
                            .is_some_and(|o| o.option.iter().any(|o| o.channel.can_be_master())),
                        "master '{}' does not support multiplayer grouping",
                        self.master
                    );

                    for slave in self.slaves.iter().map(|s| &s.device_id) {
                        anyhow::ensure!(
                            try_find_facts_by_id(&facts, &slave)?
                                .group_status
                                .zone_options
                                .as_ref()
                                .is_some_and(|o| o.option.iter().any(|o| o.channel.can_be_slave())),
                            "slave '{slave}' does not support multiplayer grouping"
                        );
                    }

                    State::RenameSlaves
                }
                State::RenameSlaves => {
                    for slave in &self.slaves {
                        try_find_client_by_id(&clients, &slave.device_id)?
                            .set_node_name(&slave.node_name)
                            .await?;
                    }

                    State::Group
                }
                State::Group => {
                    // Map slave to endpoints to add to the master node
                    let endpoints_to_add: Vec<_> = self
                        .slaves
                        .iter()
                        .map(|s| &s.device_id)
                        .filter_map(|s| try_find_client_by_id(&clients, s).ok())
                        .map(|client| client.ip_and_port())
                        .collect();

                    anyhow::ensure!(!endpoints_to_add.is_empty(), "no devices found to group");

                    // Add slaves to the master node
                    try_find_client_by_id(&clients, &self.master)?
                        .add_slaves(
                            &endpoints_to_add,
                            ZoneMode::MultiplayerGroup {
                                group_name: self.group_name.clone(),
                            },
                        )
                        .await?;

                    State::GroupWait
                }
                State::GroupWait => {
                    let facts = try_find_facts_by_id(&facts, &self.master)?;

                    if self.slaves.iter().all(|s| {
                        facts
                            .group_status
                            .zone_slave
                            .iter()
                            .find(|zs| zs.name.as_deref().is_some_and(|name| name == s.node_name))
                            .is_some()
                    }) {
                        State::ConfigureSlaves
                    } else {
                        anyhow::ensure!(
                            transition_time_point.elapsed() < Duration::from_secs(30),
                            "timeout waiting on zone slaves to become available"
                        );

                        sleep(Duration::from_secs(1)).await;
                        State::GroupWait
                    }
                }
                State::ConfigureSlaves => {
                    let client = try_find_client_by_id(&clients, &self.master)?;
                    let facts = try_find_facts_by_id(&facts, &self.master)?;

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

                    State::ConfigureMaster
                }
                State::ConfigureMaster => {
                    let client = try_find_client_by_id(&clients, &self.master)?;

                    if let Some(brightness) = self.led_brightness {
                        client.set_led_brightness(brightness).await?;
                    }
                    if let Some(audio_preset) = self.audio_preset {
                        client.set_audio_preset(audio_preset).await?;
                    }

                    State::Finished
                }
                State::Finished => break,
            };

            if statemachine != next_state {
                statemachine = next_state;
                transition_time_point = Instant::now();
            }
        }

        Ok(())
    }
}
