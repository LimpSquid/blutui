use std::collections::HashMap;
use std::fmt::Debug;
use std::time::{Duration, Instant};

use anyhow::Context;
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use strum::IntoEnumIterator;
use tokio::time::sleep;

use super::super::client::ZoneMode;
use super::super::protocol::{AudioPreset, LedBrightness};
use super::super::{MAX_VOLUME_LEVEL, MIN_VOLUME_LEVEL};
use super::common::*;
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
    Group,
    ConfigureInput,
    // NB: process this state after configuring the input. Input selection changes
    // implicitly also cause audio preset changes in bluesound devices.
    Configure,
    Finished,
}

impl State {
    fn should_gather_facts(&self) -> bool {
        match self {
            Self::Check
            | Self::UngroupSlaves
            | Self::UngroupMasters
            | Self::Group
            | Self::ConfigureInput
            | Self::Configure
            | Self::WaitForDevices => true,
            Self::Wait { .. } | Self::Finished => false,
        }
    }
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub struct GroupProfileDevice {
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

impl GroupProfileDevice {
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
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub struct GroupProfile {
    /// The master of the group
    pub master: GroupProfileDevice,
    /// One or more slaves of the group
    pub slaves: Vec<GroupProfileDevice>,
    /// The source selection of this group, if `None` use the current source selection
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceSelection>,
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

impl GroupProfile {
    pub fn validate(&self) -> anyhow::Result<()> {
        // Must be one slave in the group
        anyhow::ensure!(
            !self.slaves.is_empty(),
            "at least one slave needs to be specified"
        );

        // Validate devices
        self.slaves
            .iter()
            .chain(std::iter::once(&self.master))
            .try_for_each(GroupProfileDevice::validate)?;

        // Master cannot be specified in slaves
        anyhow::ensure!(
            self.slaves
                .iter()
                .find(|s| s.device_id == self.master.device_id)
                .is_none(),
            "master device is also specified in slaves"
        );

        // There can be no duplicate slaves
        anyhow::ensure!(
            self.slaves.iter().unique_by(|s| s.device_id).count() == self.slaves.len(),
            "duplicate slave specified"
        );

        if let Some(source) = self.source.as_ref() {
            source.validate()?;
        }

        Ok(())
    }

    #[tracing::instrument(err, skip_all)]
    pub(super) async fn apply(self, clients: SharedClientMap) -> anyhow::Result<()> {
        self.validate().context("group profile invalid")?;

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

                State::Check => {
                    if let Ok((master_ip, master_port)) =
                        try_find_client_by_id(&clients, &self.master.device_id)
                            .map(|m| m.ip_and_port())
                    {
                        let master = try_find_facts_by_id(&facts, &self.master.device_id)?;
                        if master.group_status.zone_slave.is_empty() // Must not be part of a fixed zone group
                            && self.slaves.len() == master.group_status.slave.len()
                            && self.slaves.iter().all(|s| {
                                match try_find_facts_by_id(&facts, &s.device_id) {
                                    Ok(f) => {
                                        f.group_status.am_i_slave()
                                            && f.group_status.master.as_ref().is_some_and(|m| {
                                                m.ip_addr == master_ip && m.port == master_port
                                            })
                                    }
                                    Err(_) => false,
                                }
                            })
                        {
                            // Group is already correct, skip grouping step
                            State::ConfigureInput
                        } else {
                            // Group is incorrect, try and ungroup devices
                            State::UngroupSlaves
                        }
                    } else {
                        // Master not found, try and ungroup devices
                        State::UngroupSlaves
                    }
                }
                State::UngroupSlaves => {
                    for ((master_ip, master_port), endpoints_to_remove) in self
                        .slaves
                        .iter()
                        .chain(std::iter::once(&self.master))
                        .map(|s| s.device_id)
                        .chain(self.ungroup_extra.clone().unwrap_or_default())
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
                        .chain(std::iter::once(&self.master))
                        .map(|s| s.device_id)
                        .chain(self.ungroup_extra.clone().unwrap_or_default())
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
                        .chain(std::iter::once(&self.master))
                        .map(|s| s.device_id)
                        // Wait until device is reachable
                        .filter(|device_id| try_find_facts_by_id(&facts, device_id).is_err())
                        .collect();

                    if not_found.is_empty() {
                        State::Group
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
                State::Group => {
                    // Map slave to endpoints to add to the master node
                    let endpoints_to_add: Vec<_> = self
                        .slaves
                        .iter()
                        .filter_map(|s| try_find_client_by_id(&clients, &s.device_id).ok())
                        .map(|client| client.ip_and_port())
                        .collect();

                    anyhow::ensure!(!endpoints_to_add.is_empty(), "no devices found to group");

                    // Add slaves to the master node
                    try_find_client_by_id(&clients, &self.master.device_id)?
                        .add_slaves(&endpoints_to_add, ZoneMode::Group { group_name: None })
                        .await?;

                    State::Wait {
                        duration: Duration::from_secs(5), // Give some time to settle
                        next_state: Box::new(State::ConfigureInput),
                    }
                }
                State::ConfigureInput => {
                    match self.source.as_ref() {
                        Some(SourceSelection::Input { input }) => {
                            let (master, facts) = (
                                try_find_client_by_id(&clients, &self.master.device_id)?,
                                try_find_facts_by_id(&facts, &self.master.device_id)?,
                            );
                            let play_url = facts
                                .input_selection
                                .find_input(input)
                                .map(|i| i.url.clone())
                                .ok_or_else(|| {
                                    anyhow::anyhow!(
                                        "invalid input selection, available: {}",
                                        facts.input_selection.list_inputs()
                                    )
                                })?;
                            master.play(Some(play_url)).await?;
                        }
                        Some(SourceSelection::Preset { preset_id }) => {
                            let master = try_find_client_by_id(&clients, &self.master.device_id)?;
                            master.load_preset(*preset_id).await?;
                        }
                        None => {}
                    }

                    State::Configure
                }
                State::Configure => {
                    for (client, profile) in self
                        .slaves
                        .iter()
                        .chain(std::iter::once(&self.master))
                        .filter_map(|p| {
                            Some((try_find_client_by_id(&clients, &p.device_id).ok()?, p))
                        })
                    {
                        if let Some(brightness) = profile.led_brightness {
                            client.set_led_brightness(brightness).await?;
                        }
                        if let Some(node_name) = profile.node_name.as_deref() {
                            client.set_node_name(node_name).await?;
                        }
                        if let Some(level) = profile.volume_level {
                            client.set_volume_level(level, false).await?;
                        }
                        if let Some(audio_preset) = profile.audio_preset {
                            client.set_audio_preset(audio_preset).await?;
                        }
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
