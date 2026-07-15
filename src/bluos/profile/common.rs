use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;

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
}
