use std::net::IpAddr;
use std::ops::{Deref, DerefMut};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use uuid::{Uuid, uuid};

const DEVICE_ID_NS: Uuid = uuid!("165004ca-62d8-461f-a465-c8505d3a76d4");
const GROUP_ID_NS: Uuid = uuid!("8bd1f9e8-8eb9-4627-aa33-0dfed81df620");

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(transparent)]
pub struct DeviceId(Uuid);

impl DeviceId {
    pub fn new(node_id: &[u8]) -> Self {
        Self(Uuid::new_v5(&DEVICE_ID_NS, node_id))
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
            &GROUP_ID_NS,
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

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize)]
#[serde(transparent)]
pub struct NoDebug<T>(pub T);

impl<T> std::fmt::Debug for NoDebug<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(std::any::type_name::<T>())
    }
}

impl<T> Deref for NoDebug<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> DerefMut for NoDebug<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<T> From<T> for NoDebug<T> {
    fn from(value: T) -> Self {
        Self(value)
    }
}
