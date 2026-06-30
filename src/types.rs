use std::{net::IpAddr, path::PathBuf};

use serde::{Deserialize, Serialize};
use uuid::{Uuid, uuid};

const UUID_NS: Uuid = uuid!("165004ca-62d8-461f-a465-c8505d3a76d4");

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(transparent)]
pub struct DeviceId(Uuid);

impl DeviceId {
    pub fn new(node_id: &[u8]) -> Self {
        Self(Uuid::new_v5(&UUID_NS, node_id))
    }
}

impl std::str::FromStr for DeviceId {
    type Err = uuid::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::from_str(s)?))
    }
}

impl std::fmt::Display for DeviceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl From<Uuid> for DeviceId {
    fn from(uuid: Uuid) -> Self {
        Self(uuid)
    }
}

impl From<DeviceId> for Uuid {
    fn from(device_id: DeviceId) -> Self {
        device_id.0
    }
}

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(transparent)]
pub struct GroupId(Uuid);

impl GroupId {
    pub fn new(master_ip_addr: IpAddr, master_port: u16) -> Self {
        Self(Uuid::new_v5(
            &UUID_NS,
            format!("{master_ip_addr}{master_port}").as_bytes(),
        ))
    }
}

impl std::str::FromStr for GroupId {
    type Err = uuid::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::from_str(s)?))
    }
}

impl std::fmt::Display for GroupId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl From<Uuid> for GroupId {
    fn from(uuid: Uuid) -> Self {
        Self(uuid)
    }
}

impl From<GroupId> for Uuid {
    fn from(group_id: GroupId) -> Self {
        group_id.0
    }
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(transparent)]
pub struct ProfileId(PathBuf);

impl ProfileId {
    pub fn new(path: PathBuf) -> Self {
        Self(path)
    }
}

impl std::fmt::Display for ProfileId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.to_string_lossy().fmt(f)
    }
}
