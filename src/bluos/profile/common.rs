use std::collections::HashMap;
use std::fmt::Debug;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Context;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use super::super::client::HttpClient;
use super::super::protocol::{DeviceGroupStatus, DeviceInputSelection, DeviceStatus};
use crate::types::DeviceId;

pub type ClientMap = HashMap<DeviceId, HttpClient>;
pub type SharedClientMap = Arc<RwLock<ClientMap>>;
pub type FactMap = HashMap<DeviceId, DeviceFacts>;

pub fn validate_name(node_name: &str) -> anyhow::Result<()> {
    anyhow::ensure!(!node_name.is_empty(), "node name must be atleast one char");
    anyhow::ensure!(
        node_name.len() <= 32,
        "node name must be 32 chars at maximum"
    );
    anyhow::ensure!(
        node_name.is_ascii(),
        "node name must only contain ASCII chars"
    );

    Ok(())
}

pub fn try_find_client_by_id<'a>(
    clients: &'a ClientMap,
    device_id: &'a DeviceId,
) -> anyhow::Result<&'a HttpClient> {
    clients
        .get(device_id)
        .context(format!("cannot find device {device_id}"))
}

pub fn try_find_client_by_ip_and_port(
    clients: &ClientMap,
    ip: IpAddr,
    port: u16,
) -> anyhow::Result<&HttpClient> {
    clients
        .values()
        .find(|c| c.ip_and_port() == (ip, port))
        .context(format!("cannot find device {ip}:{port}"))
}

pub fn try_find_facts_by_id<'a>(
    facts: &'a FactMap,
    device_id: &DeviceId,
) -> anyhow::Result<&'a DeviceFacts> {
    facts
        .get(device_id)
        .context(format!("cannot find device facts {device_id}"))
}

#[derive(Debug, Clone)]
#[allow(unused)]
pub struct DeviceFacts {
    pub status: DeviceStatus,
    pub group_status: DeviceGroupStatus,
    pub input_selection: DeviceInputSelection,
}

impl DeviceFacts {
    pub async fn gather_for_all(clients: ClientMap) -> anyhow::Result<HashMap<DeviceId, Self>> {
        let results = futures::stream::iter(clients)
            .map(|(id, client)| async move { (id, DeviceFacts::gather_for_one(&client).await) })
            .buffer_unordered(10)
            .collect::<Vec<_>>()
            .await;

        Ok(results
            .into_iter()
            .filter_map(|(k, v)| Some((k, v.ok()?)))
            .collect())
    }

    pub async fn gather_for_one(client: &HttpClient) -> anyhow::Result<Self> {
        let (status, group_status, input_selection) = tokio::try_join!(
            client.get_device_status(None),
            client.get_group_status(None),
            client.get_input_selection(),
        )?;

        Ok(Self {
            status,
            group_status,
            input_selection,
        })
    }
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(tag = "type")]
#[serde(rename_all = "lowercase")]
pub enum SourceSelection {
    /// Must match the `text` from one of `DeviceInputSelectionItem` (case insensitive)
    Input { input: String },
    /// Must match an existing preset ID
    Preset { preset_id: usize },
}

impl SourceSelection {
    pub fn validate(&self) -> anyhow::Result<()> {
        match &self {
            Self::Input { input } => {
                anyhow::ensure!(!input.is_empty(), "input cannot be empty");
                anyhow::ensure!(input.is_ascii(), "input contains non ASCII chars");
            }
            Self::Preset { preset_id } => anyhow::ensure!(*preset_id > 0, "preset id must be > 0"),
        }

        Ok(())
    }

    pub async fn apply(
        &self,
        clients: &ClientMap,
        facts: &FactMap,
        device_id: &DeviceId,
    ) -> anyhow::Result<()> {
        match self {
            SourceSelection::Input { input } => {
                let (device, facts) = (
                    try_find_client_by_id(clients, device_id)?,
                    try_find_facts_by_id(facts, device_id)?,
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
                device.play(Some(play_url)).await?;
            }
            SourceSelection::Preset { preset_id } => {
                let device = try_find_client_by_id(clients, device_id)?;
                device.load_preset(*preset_id).await?;
            }
        }

        Ok(())
    }
}

pub async fn ungroup_slaves_from_master(
    device_ids: impl Iterator<Item = DeviceId>,
    facts: &FactMap,
    clients: &ClientMap,
) -> anyhow::Result<()> {
    for ((master_ip, master_port), endpoints_to_remove) in device_ids
        .filter_map(|device_id| try_find_facts_by_id(facts, &device_id).ok())
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
        try_find_client_by_ip_and_port(clients, master_ip, master_port)?
            .remove_slaves(&endpoints_to_remove)
            .await?;
    }

    Ok(())
}

pub async fn ungroup_masters(
    device_ids: impl Iterator<Item = DeviceId>,
    facts: &FactMap,
    clients: &ClientMap,
) -> anyhow::Result<()> {
    for ((master_ip, master_port), endpoints_to_remove) in device_ids
        .filter_map(|device_id| try_find_facts_by_id(facts, &device_id).ok())
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
        try_find_client_by_ip_and_port(clients, master_ip, master_port)?
            .remove_slaves(&endpoints_to_remove)
            .await?;
    }

    Ok(())
}

pub trait Profile {
    fn validate(&self) -> anyhow::Result<()>;

    async fn apply(self, clients: SharedClientMap) -> anyhow::Result<()>;
}

pub(super) enum NextState<S> {
    Immediate(S),
    After(Duration, S),
    Finished,
}

pub(super) trait StateMachine {
    type State: Default + Debug + PartialEq + Send + 'static;

    /// Run a single state
    async fn run_state(
        &self,
        state: &Self::State,
        facts: &FactMap,
        clients: &ClientMap,
        transition_time_point: Instant,
    ) -> anyhow::Result<NextState<Self::State>>;

    /// Indicate wheter we should regather facts for a given state. By default
    /// for every state machine iteration we regather device facts
    fn should_regather_facts(_: &Self::State) -> bool {
        true
    }

    /// Run the state machine
    async fn run(self, clients: SharedClientMap) -> anyhow::Result<()>
    where
        Self: Sized,
    {
        let mut state = Self::State::default();
        let mut transition_time_point = Instant::now();
        let mut facts = FactMap::default();

        loop {
            let clients = clients.read().await.to_owned();

            if Self::should_regather_facts(&state) {
                facts = DeviceFacts::gather_for_all(clients.clone()).await?;
            };

            tracing::debug!(?state, ?facts, "executing state");

            let next_state = self
                .run_state(&state, &facts, &clients, transition_time_point)
                .await?;

            match next_state {
                NextState::Finished => break,
                NextState::After(duration, next_state) => {
                    tokio::time::sleep(duration).await;

                    if state != next_state {
                        state = next_state;
                        transition_time_point = Instant::now();
                    }
                }
                NextState::Immediate(next_state) => {
                    if state != next_state {
                        state = next_state;
                        transition_time_point = Instant::now();
                    }
                }
            }
        }

        Ok(())
    }
}
