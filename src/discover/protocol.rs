use std::collections::BTreeMap;
use std::net::IpAddr;
use std::ops::Deref;

use chrono::{DateTime, Utc};

use crate::types::DeviceId;

const MAGIC_WORD: &[u8; 4] = b"LSDP";
const PROTOCOL_VERSION: u8 = 1;

struct Field<'a>(&'a [u8]);

impl<'a> Field<'a> {
    fn decode(data: &'a [u8]) -> anyhow::Result<(&'a [u8], Self)> {
        anyhow::ensure!(data.len() >= 1, "insufficient data");
        let len = data[0] as usize;
        let data = &data[1..];
        anyhow::ensure!(data.len() >= len, "insufficient data");

        Ok((&data[len..], Self(&data[0..len])))
    }
}

impl<'a> Deref for Field<'a> {
    type Target = &'a [u8];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

struct Records<T>(Vec<T>);

impl<T> Records<T> {
    fn decode<F>(data: &[u8], decode: F) -> anyhow::Result<(&[u8], Self)>
    where
        F: Fn(&[u8]) -> anyhow::Result<(&[u8], T)>,
    {
        anyhow::ensure!(data.len() >= 1, "insufficient data");

        let count = data[0] as usize;
        let mut data = &data[1..];
        let mut records = Vec::new();

        for _ in 0..count {
            let (remain, record) = decode(data)?;
            records.push(record);
            data = remain;
        }

        Ok((data, Self(records)))
    }

    fn unwrap(self) -> Vec<T> {
        self.0
    }
}

impl<T> Deref for Records<T> {
    type Target = Vec<T>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DeviceClass {
    Player,
    Server,
    SecondaryPlayer,
    Testing,
    PairSlave,
    Hub,

    // Special cases
    All,
    Custom(u16),
}

impl DeviceClass {
    fn encode(self) -> [u8; 2] {
        match self {
            Self::Player => 0x0001u16.to_be_bytes(),
            Self::Server => 0x0002u16.to_be_bytes(),
            Self::SecondaryPlayer => 0x0003u16.to_be_bytes(),
            Self::Testing => 0x0004u16.to_be_bytes(),
            Self::PairSlave => 0x0006u16.to_be_bytes(),
            Self::Hub => 0x0008u16.to_be_bytes(),
            Self::All => 0xffffu16.to_be_bytes(),
            Self::Custom(id) => id.to_be_bytes(),
        }
    }

    fn decode(data: &[u8]) -> anyhow::Result<(&[u8], Self)> {
        anyhow::ensure!(data.len() >= 2, "insufficient data");

        let class = match u16::from_be_bytes(data[0..2].try_into()?) {
            0x0001 => Self::Player,
            0x0002 => Self::Server,
            0x0003 => Self::SecondaryPlayer,
            0x0004 => Self::Testing,
            0x0006 => Self::PairSlave,
            0x0008 => Self::Hub,
            0xffff => Self::All,
            id => Self::Custom(id),
        };

        Ok((&data[2..], class))
    }

    pub fn is_physical(&self) -> bool {
        matches!(
            self,
            Self::Player | Self::Server | Self::SecondaryPlayer | Self::PairSlave | Self::Hub
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryMessage {
    pub classes: Vec<DeviceClass>,
}

impl QueryMessage {
    pub fn devices(classes: &[DeviceClass]) -> anyhow::Result<Self> {
        anyhow::ensure!(
            !classes.contains(&DeviceClass::All),
            "use QueryMessage::all_devices() instead"
        );

        let mut classes = classes.to_vec();
        classes.sort();
        classes.dedup();

        anyhow::ensure!(
            classes.len() <= u8::MAX as usize,
            "no more than {} device classes can be queried",
            u8::MAX
        );

        Ok(Self { classes })
    }

    pub fn all_devices() -> Self {
        Self {
            classes: vec![DeviceClass::All],
        }
    }

    fn encode(self) -> Vec<u8> {
        let mut b = Vec::new();

        b.push(self.classes.len() as u8);
        for c in self.classes {
            b.extend(c.encode());
        }
        b
    }

    fn decode(data: &[u8]) -> anyhow::Result<(&[u8], Self)> {
        let (data, classes) = Records::decode(data, DeviceClass::decode)?;

        Ok((
            data,
            Self {
                classes: classes.unwrap(),
            },
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnnounceMessage {
    pub node_id: Vec<u8>,
    pub ip_addr: IpAddr,
    pub records: Vec<AnnounceRecord>,
}

impl AnnounceMessage {
    pub fn into_device(self, last_update: DateTime<Utc>) -> Device {
        Device {
            id: DeviceId::new(&self.node_id),
            ip_addr: self.ip_addr,
            attributes: self
                .records
                .into_iter()
                .map(|r| DeviceAttr {
                    class: r.class,
                    fields: r.fields,
                })
                .collect(),
            last_update,
        }
    }

    fn encode(self) -> Vec<u8> {
        let mut b = Vec::new();
        b.push(self.node_id.len() as u8);
        b.extend(self.node_id);
        match self.ip_addr {
            IpAddr::V4(ip) => {
                b.push(4u8);
                b.extend(ip.octets());
            }
            IpAddr::V6(ip) => {
                b.push(16u8);
                b.extend(ip.octets());
            }
        }
        b.push(self.records.len() as u8);
        self.records.into_iter().for_each(|r| b.extend(r.encode()));
        b
    }

    fn decode(data: &[u8]) -> anyhow::Result<(&[u8], Self)> {
        anyhow::ensure!(data.len() >= 1, "insufficient data");

        let (data, node_id) = Field::decode(data)?;
        let (data, ip_addr) = Field::decode(data)?;
        let ip_addr = match ip_addr.len() {
            4 => TryInto::<[u8; 4]>::try_into(*ip_addr)?.into(),
            16 => TryInto::<[u8; 16]>::try_into(*ip_addr)?.into(),
            _ => anyhow::bail!("unknown IP addr length"),
        };
        let (data, records) = Records::decode(data, AnnounceRecord::decode)?;

        Ok((
            data,
            Self {
                node_id: node_id.to_vec(),
                ip_addr,
                records: records.unwrap(),
            },
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnnounceRecord {
    pub class: DeviceClass,
    pub fields: BTreeMap<String, String>,
}

impl AnnounceRecord {
    fn encode(self) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend(self.class.encode());
        b.push(self.fields.len() as u8);
        for (key, value) in self.fields {
            b.push(key.len() as u8);
            b.extend(key.as_bytes());
            b.push(value.len() as u8);
            b.extend(value.as_bytes());
        }
        b
    }

    fn decode(data: &[u8]) -> anyhow::Result<(&[u8], Self)> {
        let (data, class) = DeviceClass::decode(data)?;
        let (data, records) = Records::decode(data, |data| {
            let (remain, key) = Field::decode(data)?;
            let (remain, value) = Field::decode(remain)?;

            Ok((
                remain,
                (
                    std::str::from_utf8(*key)?.to_owned(),
                    std::str::from_utf8(*value)?.to_owned(),
                ),
            ))
        })?;

        Ok((
            data,
            Self {
                class,
                fields: records.unwrap().into_iter().collect(),
            },
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeleteMessage {
    pub node_id: Vec<u8>,
    pub classes: Vec<DeviceClass>,
}

impl DeleteMessage {
    pub fn device_id(&self) -> DeviceId {
        DeviceId::new(&self.node_id)
    }

    fn decode(data: &[u8]) -> anyhow::Result<(&[u8], Self)> {
        anyhow::ensure!(data.len() >= 1, "insufficient data");

        let (data, node_id) = Field::decode(data)?;
        let (data, classes) = Records::decode(data, DeviceClass::decode)?;

        Ok((
            data,
            Self {
                node_id: node_id.to_vec(),
                classes: classes.unwrap(),
            },
        ))
    }
}

#[derive(Debug, Clone)]
pub struct DeviceAttr {
    pub class: DeviceClass,
    pub fields: BTreeMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct Device {
    pub id: DeviceId,
    pub ip_addr: IpAddr,
    pub attributes: Vec<DeviceAttr>,
    pub last_update: DateTime<Utc>,
}

impl PartialEq for Device {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for Device {}

impl std::hash::Hash for Device {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl From<&Device> for DeviceId {
    fn from(device: &Device) -> Self {
        device.id
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    pub protocol_version: u8,
}

impl Header {
    fn new() -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
        }
    }

    const fn len() -> usize {
        size_of::<u8>() // header length
            + size_of_val(MAGIC_WORD) // magic word
            + size_of::<u8>() // protocol version
    }

    fn encode(self) -> Vec<u8> {
        let mut b = Vec::new();
        b.push(Self::len() as u8);
        b.extend_from_slice(MAGIC_WORD);
        b.push(self.protocol_version);
        b
    }

    fn decode(data: &[u8]) -> anyhow::Result<(&[u8], Self)> {
        anyhow::ensure!(data.len() >= Self::len(), "insufficient data");

        let length = data[0];
        let magic_word = &data[1..=4];
        let protocol_version = data[5];

        anyhow::ensure!(length == Self::len() as u8, "incorrect length");
        anyhow::ensure!(magic_word == MAGIC_WORD, "incorrect magic word");
        anyhow::ensure!(
            protocol_version == PROTOCOL_VERSION,
            "incorrect protocol version"
        );

        Ok((&data[6..], Self { protocol_version }))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Message {
    Announce(AnnounceMessage),
    Delete(DeleteMessage),
    Query(QueryMessage),
}

impl Message {
    fn type_id(&self) -> u8 {
        match self {
            Self::Announce(_) => 0x41,
            Self::Delete(_) => 0x44,
            Self::Query(_) => 0x51,
        }
    }

    fn encode(self) -> anyhow::Result<Vec<u8>> {
        let type_id = self.type_id();
        let message = match self {
            Self::Announce(inner) => inner.encode(),
            Self::Delete(_) => anyhow::bail!("cannot encode delete message"),
            Self::Query(inner) => inner.encode(),
        };

        let mut b = Vec::new();
        b.push(2 + message.len() as u8);
        b.push(type_id);
        b.extend(message);
        Ok(b)
    }

    fn decode(data: &[u8]) -> anyhow::Result<(&[u8], Self)> {
        anyhow::ensure!(data.len() >= 2, "insufficient data");

        // let message_length = data[0];
        let type_id = data[1];
        let (data, message) = match type_id {
            0x41 => {
                let (data, message) = AnnounceMessage::decode(&data[2..])?;
                (data, Self::Announce(message))
            }
            0x44 => {
                let (data, message) = DeleteMessage::decode(&data[2..])?;
                (data, Self::Delete(message))
            }
            0x51 => {
                let (data, message) = QueryMessage::decode(&data[2..])?;
                (data, Self::Query(message))
            }
            _ => anyhow::bail!("invalid message type {type_id:#04x}"),
        };

        Ok((data, message))
    }
}

impl From<QueryMessage> for Message {
    fn from(query: QueryMessage) -> Self {
        Self::Query(query)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Packet {
    pub header: Header,
    pub message: Message,
}

impl Packet {
    pub fn new(message: impl Into<Message>) -> Self {
        Self {
            header: Header::new(),
            message: message.into(),
        }
    }

    pub fn encode(self) -> anyhow::Result<Vec<u8>> {
        let mut b = Vec::new();
        b.extend(self.header.encode());
        b.extend(self.message.encode()?);
        Ok(b)
    }

    pub fn decode(data: &[u8]) -> anyhow::Result<Packet> {
        let (data, header) = Header::decode(data)?;
        let (_, message) = Message::decode(data)?;
        Ok(Self { header, message })
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn packet_decode_query_message() {
        let bytes = [
            0x06, 0x4c, 0x53, 0x44, 0x50, 0x01, 0x05, 0x51, 0x01, 0xff, 0xff, 0x00,
        ];

        let got = Packet::decode(&bytes).unwrap();
        let expected = Packet::new(Message::Query(QueryMessage {
            classes: vec![DeviceClass::All],
        }));

        assert_eq!(got, expected);
    }

    #[test]
    fn packet_decode_announce_message() {
        let bytes = [
            0x06, 0x4C, 0x53, 0x44, 0x50, 0x01, 0x6A, 0x41, 0x06, 0x90, 0x56, 0x82, 0x0E, 0x1B,
            0x00, 0x04, 0x0A, 0x00, 0x01, 0x24, 0x02, 0x00, 0x01, 0x05, 0x04, 0x6E, 0x61, 0x6D,
            0x65, 0x0A, 0x53, 0x45, 0x41, 0x4C, 0x50, 0x4C, 0x41, 0x59, 0x45, 0x52, 0x04, 0x70,
            0x6F, 0x72, 0x74, 0x05, 0x31, 0x31, 0x30, 0x30, 0x30, 0x05, 0x6D, 0x6F, 0x64, 0x65,
            0x6C, 0x04, 0x43, 0x33, 0x38, 0x38, 0x07, 0x76, 0x65, 0x72, 0x73, 0x69, 0x6F, 0x6E,
            0x06, 0x33, 0x2E, 0x31, 0x36, 0x2E, 0x35, 0x02, 0x7A, 0x73, 0x01, 0x30, 0x00, 0x04,
            0x02, 0x04, 0x6E, 0x61, 0x6D, 0x65, 0x0A, 0x53, 0x45, 0x41, 0x4C, 0x50, 0x4C, 0x41,
            0x59, 0x45, 0x52, 0x04, 0x70, 0x6F, 0x72, 0x74, 0x05, 0x31, 0x31, 0x34, 0x33, 0x31,
        ];

        let got = Packet::decode(&bytes).unwrap();
        let expected = Packet::new(Message::Announce(AnnounceMessage {
            node_id: vec![144, 86, 130, 14, 27, 0],
            ip_addr: "10.0.1.36".parse().unwrap(),
            records: vec![
                AnnounceRecord {
                    class: DeviceClass::Player,
                    fields: BTreeMap::from([
                        ("model".to_string(), "C388".to_string()),
                        ("name".to_string(), "SEALPLAYER".to_string()),
                        ("zs".to_string(), "0".to_string()),
                        ("port".to_string(), "11000".to_string()),
                        ("version".to_string(), "3.16.5".to_string()),
                    ]),
                },
                AnnounceRecord {
                    class: DeviceClass::Testing,
                    fields: BTreeMap::from([
                        ("name".to_string(), "SEALPLAYER".to_string()),
                        ("port".to_string(), "11431".to_string()),
                    ]),
                },
            ],
        }));

        assert_eq!(got, expected);
    }

    #[test]
    fn malformed_packet() {
        assert!(Packet::decode(&[]).is_err());
        assert!(Packet::decode(&[0x00]).is_err());
        assert!(Packet::decode(&[0x00, 0x00]).is_err());
        assert!(Packet::decode(&[0xff, 0xff]).is_err());
        assert!(
            Packet::decode(&[
                0x06, 0x4c, 0x53, 0x44, 0x50, 0x01, 0x05, 0x51, 0x02, 0xff, 0xff, 0x00
            ])
            .is_err()
        );
        assert!(
            Packet::decode(&[
                0x06, 0x4c, 0x53, 0x44, 0x50, 0x01, 0x05, 0x52, 0x01, 0xff, 0xff, 0x00
            ])
            .is_err()
        );
        assert!(
            Packet::decode(&[
                0x06, 0x4c, 0x53, 0x44, 0x50, 0x02, 0x05, 0x51, 0x01, 0xff, 0xff, 0x00
            ])
            .is_err()
        );
        assert!(
            Packet::decode(&[
                0x05, 0x4c, 0x53, 0x44, 0x50, 0x01, 0x05, 0x51, 0x01, 0xff, 0xff, 0x00
            ])
            .is_err()
        );
        assert!(
            Packet::decode(&[
                0x06, 0x4d, 0x53, 0x44, 0x50, 0x01, 0x05, 0x51, 0x01, 0xff, 0xff, 0x00
            ])
            .is_err()
        );
        assert!(
            Packet::decode(&[
                0x06, 0x4c, 0x53, 0x44, 0x51, 0x01, 0x05, 0x51, 0x01, 0xff, 0xff, 0x00
            ])
            .is_err()
        );
    }
}
