use std::time::Duration;
use std::{collections::HashMap, net::IpAddr};

use itertools::Itertools;
use reqwest::{Client, RequestBuilder, Url};
use scraper::{Html, Selector};
use serde::Serialize;

use crate::discover::Device;

use super::protocol::*;

// In seconds
const DEFAULT_POLL_TIMEOUT: u8 = 60;
// In seconds
const POLL_GRACE_PERIOD: u64 = 2;
// In seconds
const REQUEST_TIMEOUT: u64 = 2;
// In seconds
const REQUEST_TIMEOUT_LONG: u64 = 5;
const DEFAULT_DEVICE_PORT: u16 = 11000;

trait RequestBuilderExt {
    fn poll_opts(self, opts: Option<PollOpts>) -> Self;
}

impl RequestBuilderExt for RequestBuilder {
    fn poll_opts(self, opts: Option<PollOpts>) -> Self {
        if let Some(opts) = opts {
            self.query(&opts)
                .timeout(Duration::from_secs(opts.timeout as u64 + POLL_GRACE_PERIOD))
        } else {
            self
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PollOpts {
    /// The etag attribute from the previous poll response
    pub etag: String,
    /// Poll timeout in seconds
    pub timeout: u8,
}

impl PollOpts {
    pub fn new(etag: &str) -> Self {
        Self {
            etag: etag.to_owned(),
            timeout: DEFAULT_POLL_TIMEOUT,
        }
    }
}

#[derive(Clone)]
pub struct HttpClient {
    ip_addr: IpAddr,
    port: u16,
    client: Client,
}

impl HttpClient {
    pub fn new(ip_addr: IpAddr, port: u16) -> Self {
        let client = Client::new();

        Self {
            ip_addr,
            port,
            client,
        }
    }

    pub fn ip_and_port(&self) -> (IpAddr, u16) {
        (self.ip_addr, self.port)
    }

    pub fn from_device(device: &Device) -> Self {
        // NB: We assume that the first `port` field of a physical device is the port used for the API communication
        let port = device
            .attributes
            .iter()
            .filter(|device| device.class.is_physical())
            .filter_map(|device| device.fields.get("port"))
            .filter_map(|port| port.parse().ok())
            .next()
            .unwrap_or(DEFAULT_DEVICE_PORT);

        Self::new(device.ip_addr, port)
    }

    pub async fn get_device_status(
        &self,
        poll_opts: Option<PollOpts>,
    ) -> anyhow::Result<DeviceStatus> {
        let response = self
            .client
            .get(self.api_path("Status")?)
            .poll_opts(poll_opts)
            .send()
            .await?
            .text()
            .await?;
        let device_status = quick_xml::de::from_str(&response)?;

        Ok(device_status)
    }

    pub async fn get_group_status(
        &self,
        poll_opts: Option<PollOpts>,
    ) -> anyhow::Result<DeviceGroupStatus> {
        let response = self
            .client
            .get(self.api_path("SyncStatus")?)
            .poll_opts(poll_opts)
            .send()
            .await?
            .text()
            .await?;
        let group_status = quick_xml::de::from_str(&response)?;

        Ok(group_status)
    }

    pub async fn get_volume(&self, poll_opts: Option<PollOpts>) -> anyhow::Result<DeviceVolume> {
        let response = self
            .client
            .get(self.api_path("Volume")?)
            .poll_opts(poll_opts)
            .send()
            .await?
            .text()
            .await?;
        let device_volume = quick_xml::de::from_str(&response)?;

        Ok(device_volume)
    }

    pub async fn get_diagnostics(&self) -> anyhow::Result<DeviceDiagnostics> {
        let response = self
            .client
            .get(self.web_path("diagnostics")?)
            .timeout(Duration::from_secs(REQUEST_TIMEOUT))
            .send()
            .await?
            .text()
            .await?;
        let html = Html::parse_document(&response);
        let key_selector = Selector::parse(r#"div.ui-block-a"#)
            .map_err(|_| anyhow::anyhow!("invalid selector"))?;
        let value_selector = Selector::parse(r#"div.ui-block-b"#)
            .map_err(|_| anyhow::anyhow!("invalid selector"))?;
        let mut fields: HashMap<_, _> = html
            .select(&key_selector)
            .zip(html.select(&value_selector))
            .map(|(key, value)| {
                (
                    key.inner_html().to_ascii_lowercase(),
                    value.inner_html().trim().to_owned(),
                )
            })
            .collect();

        Ok(DeviceDiagnostics {
            connected_to_network: fields.remove("connected to network:"),
            signal_strength: fields.remove("signal strength:"),
            uptime: fields.remove("uptime:"),
        })
    }

    pub async fn get_input_selection(&self) -> anyhow::Result<DeviceInputSelection> {
        let response = self
            .client
            .get(self.api_path("RadioBrowse")?)
            .query(&[("service", "Capture")])
            .timeout(Duration::from_secs(REQUEST_TIMEOUT_LONG))
            .send()
            .await?
            .text()
            .await?;
        let input_selection = quick_xml::de::from_str(&response)?;

        Ok(input_selection)
    }

    pub async fn get_audio_preset(&self, endpoint: &str) -> anyhow::Result<DeviceAudioPreset> {
        anyhow::ensure!(!endpoint.is_empty(), "endpoint cannot be empty");

        let response = self
            .client
            .get(self.api_path(endpoint.strip_prefix("/").unwrap_or(endpoint))?)
            .timeout(Duration::from_secs(REQUEST_TIMEOUT))
            .send()
            .await?
            .text()
            .await?;
        let device_audio_preset = quick_xml::de::from_str(&response)?;

        Ok(device_audio_preset)
    }

    pub async fn set_audio_preset(
        &self,
        setting_name: &str,
        setting_value: &str,
        endpoint: &str,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(!setting_name.is_empty(), "setting name cannot be empty");
        anyhow::ensure!(!setting_value.is_empty(), "setting value cannot be empty");
        anyhow::ensure!(!endpoint.is_empty(), "endpoint cannot be empty");

        self.client
            .post(self.api_path(endpoint.strip_prefix("/").unwrap_or(endpoint))?)
            .form(&[(setting_name, setting_value)])
            .timeout(Duration::from_secs(REQUEST_TIMEOUT))
            .send()
            .await?
            .text()
            .await?;

        Ok(())
    }

    pub async fn set_volume_level(
        &self,
        level: u8,
        tell_slaves: bool,
    ) -> anyhow::Result<DeviceVolume> {
        let response = self
            .client
            .get(self.api_path("Volume")?)
            .query(&[
                ("level", format!("{}", level.clamp(0, 100))),
                ("tell_slaves", format!("{}", tell_slaves as u8)),
            ])
            .timeout(Duration::from_secs(REQUEST_TIMEOUT))
            .send()
            .await?
            .text()
            .await?;
        let device_volume = quick_xml::de::from_str(&response)?;

        Ok(device_volume)
    }

    pub async fn set_led_brightness(&self, brightness: LedBrightness) -> anyhow::Result<()> {
        self.client
            .post(self.api_path("setting")?)
            .form(&[("ledbrightness", brightness.to_string())])
            .timeout(Duration::from_secs(REQUEST_TIMEOUT))
            .send()
            .await?;

        Ok(())
    }

    pub async fn set_node_name(&self, name: &str) -> anyhow::Result<()> {
        self.client
            .post(self.api_path("Name")?)
            .form(&[("nodename", name)])
            .timeout(Duration::from_secs(REQUEST_TIMEOUT))
            .send()
            .await?;

        Ok(())
    }

    pub async fn volume_step(
        &self,
        db_step: f32,
        tell_slaves: bool,
    ) -> anyhow::Result<DeviceVolume> {
        let response = self
            .client
            .get(self.api_path("Volume")?)
            .query(&[
                ("db", format!("{:.2}", db_step)),
                ("tell_slaves", format!("{}", tell_slaves as u8)),
            ])
            .timeout(Duration::from_secs(REQUEST_TIMEOUT))
            .send()
            .await?
            .text()
            .await?;
        let device_volume = quick_xml::de::from_str(&response)?;

        Ok(device_volume)
    }

    pub async fn mute(&self, on: bool) -> anyhow::Result<DeviceVolume> {
        let response = self
            .client
            .get(self.api_path("Volume")?)
            .query(&[("mute", on)])
            .timeout(Duration::from_secs(REQUEST_TIMEOUT))
            .send()
            .await?
            .text()
            .await?;
        let device_volume = quick_xml::de::from_str(&response)?;

        Ok(device_volume)
    }

    pub async fn load_preset(&self, preset_id: usize) -> anyhow::Result<()> {
        anyhow::ensure!(preset_id > 0, "preset id must be > 0");

        self.client
            .get(self.api_path("Preset")?)
            .query(&[("id", preset_id)])
            .timeout(Duration::from_secs(REQUEST_TIMEOUT))
            .send()
            .await?;

        Ok(())
    }

    pub async fn play(&self, url_encoded: Option<String>) -> anyhow::Result<()> {
        self.client
            .get(self.api_path(&match url_encoded {
                Some(url) => format!("Play?url={url}"),
                None => "Play".to_string(),
            })?)
            .timeout(Duration::from_secs(REQUEST_TIMEOUT))
            .send()
            .await?;

        Ok(())
    }

    pub async fn pause(&self) -> anyhow::Result<()> {
        self.client
            .get(self.api_path("Pause")?)
            .timeout(Duration::from_secs(REQUEST_TIMEOUT))
            .send()
            .await?;

        Ok(())
    }

    pub async fn stop(&self) -> anyhow::Result<()> {
        self.client
            .get(self.api_path("Stop")?)
            .timeout(Duration::from_secs(REQUEST_TIMEOUT))
            .send()
            .await?;

        Ok(())
    }

    pub async fn skip(&self) -> anyhow::Result<()> {
        self.client
            .get(self.api_path("Skip")?)
            .timeout(Duration::from_secs(REQUEST_TIMEOUT))
            .send()
            .await?;

        Ok(())
    }

    pub async fn back(&self) -> anyhow::Result<()> {
        self.client
            .get(self.api_path("Back")?)
            .timeout(Duration::from_secs(REQUEST_TIMEOUT))
            .send()
            .await?;

        Ok(())
    }

    pub async fn add_slaves(
        &self,
        endpoints: &[(IpAddr, u16)],
    ) -> anyhow::Result<Vec<DeviceGroupSlave>> {
        anyhow::ensure!(!endpoints.is_empty(), "no endpoints provided");

        let response = self
            .client
            .get(self.api_path("AddSlave")?)
            .query(&[
                ("slaves", endpoints.iter().map(|(ip, _)| ip).join(",")),
                ("ports", endpoints.iter().map(|(_, port)| port).join(",")),
            ])
            .timeout(Duration::from_secs(REQUEST_TIMEOUT))
            .send()
            .await?
            .text()
            .await?;

        #[derive(serde::Deserialize)]
        struct ListResponse {
            slave: Vec<DeviceGroupSlave>,
        }
        let response: ListResponse = quick_xml::de::from_str(&response)?;

        Ok(response.slave)
    }

    pub async fn remove_slaves(
        &self,
        endpoints: &[(IpAddr, u16)],
    ) -> anyhow::Result<DeviceGroupStatus> {
        anyhow::ensure!(!endpoints.is_empty(), "no endpoints provided");

        let response = self
            .client
            .get(self.api_path("RemoveSlave")?)
            .query(&[
                ("slaves", endpoints.iter().map(|(ip, _)| ip).join(",")),
                ("ports", endpoints.iter().map(|(_, port)| port).join(",")),
            ])
            .timeout(Duration::from_secs(REQUEST_TIMEOUT))
            .send()
            .await?
            .text()
            .await?;

        let group_status = quick_xml::de::from_str(&response)?;

        Ok(group_status)
    }

    fn api_path(&self, endpoint: &str) -> anyhow::Result<Url> {
        let base_url: Url = format!("http://{}:{}/", self.ip_addr, self.port).parse()?;
        Ok(base_url.join(endpoint)?)
    }

    fn web_path(&self, endpoint: &str) -> anyhow::Result<Url> {
        let base_url: Url = format!("http://{}/", self.ip_addr).parse()?;
        Ok(base_url.join(endpoint)?)
    }
}
