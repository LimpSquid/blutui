// Terminal User Interface compatible with BluOS
// Copyright (C) 2026 LimpSquid
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.
use std::collections::BTreeMap;
use std::convert::Infallible;
use std::net::IpAddr;
use std::str::FromStr;

use clap::Parser;
use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};
use reqwest::{Client, Request};
use serde::Serialize;

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
enum NodeType {
    Setting,
    MenuGroup,
    Text,
    #[serde(untagged)]
    Other(String),
}

impl FromStr for NodeType {
    type Err = Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "setting" => Ok(Self::Setting),
            "menuGroup" => Ok(Self::MenuGroup),
            s => Ok(Self::Other(s.to_owned())),
        }
    }
}

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// The IP address of the device to crawl
    #[arg(short, long)]
    ip: IpAddr,
    /// The port of the device to crawl
    #[arg(short, long, default_value_t = 11000)]
    port: u16,
    /// Output in JSON instead
    #[arg(short, long, default_value_t = false)]
    json: bool,
}

struct HttpClient {
    client: Client,
    ip: IpAddr,
    port: u16,
    requests: Vec<Request>,
}

impl HttpClient {
    fn new(ip: IpAddr, port: u16) -> Self {
        Self {
            client: Client::new(),
            ip,
            port,
            requests: vec![],
        }
    }

    async fn get_settings_xml(&mut self, id: Option<&str>) -> anyhow::Result<String> {
        let url = format!("http://{}:{}/Settings", self.ip, self.port);
        let request = self
            .client
            .get(&url)
            .query(&id.map(|id| [("id", id)]))
            .build()?;
        self.requests.push(request.try_clone().unwrap());

        let xml = self
            .client
            .execute(request)
            .await?
            .error_for_status()?
            .text()
            .await?;

        Ok(xml)
    }
}

#[derive(Debug, Serialize)]
struct Node {
    r#type: NodeType,
    attributes: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    children: Vec<Node>,
}

#[derive(Debug, Serialize)]
struct Output {
    urls: Vec<String>,
    tree: Vec<Node>,
}

fn collect_attributes(
    reader: &Reader<&[u8]>,
    event: &BytesStart,
) -> anyhow::Result<BTreeMap<String, String>> {
    event
        .attributes()
        .filter_map(|a| a.ok())
        .map(|a| {
            let key = reader.decoder().decode(a.key.as_ref())?.into_owned();
            let value = reader.decoder().decode(&a.value)?.into_owned();
            Ok((key, value))
        })
        .collect()
}

fn parse_children(reader: &mut Reader<&[u8]>) -> anyhow::Result<Vec<Node>> {
    let mut nodes = Vec::new();
    loop {
        match reader.read_event()? {
            Event::Start(event) => {
                let children = parse_children(reader)?;
                nodes.push(Node {
                    r#type: reader.decoder().decode(event.name().as_ref())?.parse()?,
                    attributes: collect_attributes(reader, &event)?,
                    text: None,
                    children,
                });
            }
            Event::Empty(event) => {
                nodes.push(Node {
                    r#type: reader.decoder().decode(event.name().as_ref())?.parse()?,
                    attributes: collect_attributes(reader, &event)?,
                    text: None,
                    children: vec![],
                });
            }
            Event::Text(event) => {
                let text = event.decode()?.trim().to_owned();
                if !text.is_empty() {
                    nodes.push(Node {
                        r#type: NodeType::Text,
                        attributes: BTreeMap::new(),
                        text: Some(text),
                        children: vec![],
                    });
                }
            }
            Event::End(_) => break,
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(nodes)
}

fn parse_xml(xml: &str) -> anyhow::Result<Vec<Node>> {
    let mut reader = Reader::from_str(xml);
    parse_children(&mut reader)
}

async fn crawl_nodes(nodes: &mut [Node], client: &mut HttpClient) -> anyhow::Result<()> {
    for node in nodes.iter_mut() {
        if node.r#type == NodeType::MenuGroup {
            if let Some(id) = node.attributes.get("id") {
                let xml = client.get_settings_xml(Some(id.as_str())).await?;
                let mut menu_group_nodes = parse_xml(&xml)?;

                if let Some(menu_group) = menu_group_nodes.first_mut() {
                    node.children = menu_group
                        .children
                        .iter_mut()
                        .find(|n| {
                            n.r#type == NodeType::MenuGroup
                                && n.attributes
                                    .get("id")
                                    .map(|s| s.as_str())
                                    .is_some_and(|s| s == id)
                        })
                        .map(|node| std::mem::take(&mut node.children))
                        .unwrap_or_else(|| std::mem::take(&mut menu_group.children));
                }
            }
        }

        Box::pin(crawl_nodes(&mut node.children, client)).await?;
    }
    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let mut client = HttpClient::new(args.ip, args.port);

    let xml = client.get_settings_xml(None).await?;
    let mut tree = parse_xml(&xml)?;
    crawl_nodes(&mut tree, &mut client).await?;

    let output = Output {
        tree,
        urls: client
            .requests
            .into_iter()
            .map(|r| r.url().to_string())
            .collect(),
    };
    if args.json {
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        println!("{}", yaml_serde::to_string(&output)?);
    }

    Ok(())
}
