use std::net::IpAddr;

use itertools::Itertools;
use serde::{Deserialize, Deserializer, Serialize, de};
use strum_macros::Display;

use crate::serde::number::StrU16;
use crate::types::GroupId;

fn deserialize_device_id<'de, D>(deserializer: D) -> Result<(IpAddr, u16), D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    let (ip_str, port_str) = s
        .rsplit_once(':')
        .ok_or_else(|| de::Error::custom("missing ':' in ID field"))?;

    let ip = ip_str
        .parse()
        .map_err(|e| de::Error::custom(format!("invalid IP address: {e}")))?;

    let port = port_str
        .parse()
        .map_err(|e| de::Error::custom(format!("invalid port: {e}")))?;

    Ok((ip, port))
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum DeviceState {
    Play,
    Pause,
    Stop,
    Connecting,
    Stream,
    #[serde(other)]
    Unknown,
}

impl DeviceState {
    pub fn is_playing(&self) -> bool {
        matches!(self, Self::Play | Self::Stream | Self::Connecting)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct DeviceStatus {
    #[serde(rename = "@etag")]
    pub etag: String,
    pub volume: i32,
    pub db: f32,
    pub title1: Option<String>,
    #[serde(default)]
    pub title2: Option<String>,
    pub state: DeviceState,
    pub service: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DeviceVolume {
    #[serde(rename = "@etag")]
    pub etag: String,
    #[serde(rename = "@db")]
    pub db: f32,
    #[serde(rename = "@offsetDb")]
    pub offset_db: f32,
    #[serde(rename = "@mute")]
    pub mute: bool,
    #[serde(rename = "@source")]
    pub source: Option<String>,
    #[serde(rename = "$text")]
    pub volume: i32,
}
#[derive(Debug, Clone, Deserialize)]
pub struct DeviceGroupMaster {
    #[serde(rename = "$text")]
    pub ip_addr: IpAddr,
    #[serde(rename = "@port")]
    pub port: u16,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DeviceGroupSlave {
    #[serde(rename = "@id")]
    pub ip_addr: IpAddr,
    #[serde(rename = "@port")]
    pub port: StrU16,
    #[serde(rename = "@name")]
    pub name: Option<String>,
    #[serde(rename = "@model")]
    pub model: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DeviceGroupZoneSlave {
    #[serde(rename = "@id")]
    pub ip_addr: IpAddr,
    #[serde(rename = "@port")]
    pub port: StrU16,
    #[serde(rename = "@name")]
    pub name: Option<String>,
    #[serde(rename = "@modelName")]
    pub model: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AudioPresetUrl {
    #[serde(rename = "@url")]
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, strum_macros::Display)]
#[serde(rename_all = "snake_case")]
pub enum ZoneChannel {
    // Soundbar, powernode, etc
    Front,
    // Sattelite speaker like pulse flex, pulse m
    Left,
    // Sattelite speaker like pulse flex, pulse m
    Right,
    // Sattelite speaker like pulse flex, pulse m
    SideLeft,
    // Sattelite speaker like pulse flex, pulse m
    SideRight,
    // Powernode, etc
    Side,
    #[serde(other)]
    Unknown,
}

impl ZoneChannel {
    pub fn can_be_master(&self) -> bool {
        matches!(self, Self::Front)
    }

    pub fn can_be_slave(&self) -> bool {
        matches!(
            self,
            Self::Left | Self::Right | Self::SideLeft | Self::SideRight | Self::Front
        )
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct DeviceZoneOption {
    #[serde(rename = "$text")]
    pub channel: ZoneChannel,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DeviceZoneOptions {
    #[serde(default)]
    pub option: Vec<DeviceZoneOption>,
}

impl DeviceZoneOptions {
    pub fn is_master_capable(&self) -> bool {
        self.option.iter().any(|o| o.channel.can_be_master())
    }

    pub fn is_slave_capable(&self) -> bool {
        self.option.iter().any(|o| o.channel.can_be_slave())
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct DeviceGroupStatus {
    #[serde(rename = "@etag")]
    pub etag: String,
    #[serde(rename = "@brand")]
    pub brand: Option<String>,
    #[serde(rename = "@model")]
    pub model: String,
    #[serde(rename = "@name")]
    pub name: Option<String>,
    #[serde(rename = "@mac")]
    pub mac_address: Option<String>,
    pub master: Option<DeviceGroupMaster>,
    #[serde(default)]
    pub slave: Vec<DeviceGroupSlave>,
    #[serde(rename = "zoneSlave", default)]
    pub zone_slave: Vec<DeviceGroupZoneSlave>,
    #[serde(rename = "@id", deserialize_with = "deserialize_device_id")]
    pub id: (IpAddr, u16),
    #[serde(rename = "audioPresetUrl")]
    pub audio_preset_url: Option<AudioPresetUrl>,
    #[serde(rename = "zoneOptions")]
    pub zone_options: Option<DeviceZoneOptions>,
}

impl DeviceGroupStatus {
    pub fn am_i_slave(&self) -> bool {
        self.master.is_some()
    }

    pub fn am_i_zone_slave(&self) -> bool {
        self.am_i_slave() && self.zone_options.is_none()
    }

    pub fn am_i_master(&self) -> bool {
        !self.slave.is_empty() || !self.zone_slave.is_empty()
    }

    pub fn group_id(&self) -> Option<GroupId> {
        if let Some(ref master) = self.master {
            Some(GroupId::new(master.ip_addr, master.port))
        } else if self.am_i_master() {
            let (ip_addr, port) = self.id;
            Some(GroupId::new(ip_addr, port))
        } else {
            None
        }
    }
}

#[derive(Debug, Clone)]
pub struct DeviceDiagnostics {
    pub connected_to_network: Option<String>,
    pub signal_strength: Option<String>,
    pub uptime: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DeviceAudioPresetValue {
    #[serde(rename = "@displayName")]
    pub display_name: String,
    #[serde(rename = "@name")]
    pub name: String,
    #[serde(rename = "@icon")]
    pub icon: Option<String>,
    #[serde(rename = "@iconActive")]
    pub icon_active: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DeviceAudioPreset {
    #[serde(rename = "@displayName")]
    pub display_name: String,
    #[serde(rename = "@name")]
    pub name: String,
    #[serde(rename = "@class")]
    pub class: Option<String>,
    #[serde(rename = "@url")]
    pub url: String,
    #[serde(rename = "@value")]
    pub value: String,
    #[serde(rename = "value", default)]
    pub values: Vec<DeviceAudioPresetValue>,
}

impl DeviceAudioPreset {
    pub fn find_preset(&self, selection: &str) -> Option<&DeviceAudioPresetValue> {
        self.values
            .iter()
            .find(|v| v.display_name.eq_ignore_ascii_case(selection))
    }

    pub fn list_presets(&self) -> String {
        self.values
            .iter()
            .map(|i| i.display_name.to_ascii_lowercase())
            .join(", ")
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct DeviceInputSelection {
    #[serde(default)]
    pub item: Vec<DeviceInputSelectionItem>,
}

impl DeviceInputSelection {
    pub fn find_input(&self, selection: &str) -> Option<&DeviceInputSelectionItem> {
        self.item
            .iter()
            .find(|v| v.text.eq_ignore_ascii_case(selection))
    }

    pub fn list_inputs(&self) -> String {
        self.item
            .iter()
            .map(|i| i.text.to_ascii_lowercase())
            .join(", ")
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct DeviceInputSelectionItem {
    #[serde(rename = "@text")]
    pub text: String,
    #[serde(rename = "@URL")]
    pub url: String,
}

#[derive(
    Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize, Display,
)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum LedBrightness {
    Default,
    Dim,
    Off,
}
