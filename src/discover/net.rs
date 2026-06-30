use std::collections::HashSet;
use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

use chrono::Utc;
use netdev::get_default_interface;
use tokio::net::UdpSocket;
use tokio::sync::{broadcast, mpsc};
use tokio::time::{interval, sleep};

use super::protocol::*;
use crate::event::{Event, EventBus};

const LSDP_PORT: u16 = 11430;
const STALE_DURATION: Duration = Duration::from_mins(3);
const REFRESH_INTERVAL_SLOW: Duration = Duration::from_secs(90);
const REFRESH_INTERVAL_FAST: Duration = Duration::from_secs(5);
const REFRESH_REPEAT_N_TIMES: usize = 3;
const REFRESH_REPEAT_INTERVAL: Duration = Duration::from_secs(1);

fn get_default_broadcast_address() -> anyhow::Result<IpAddr> {
    let mut default_iface = get_default_interface().map_err(|e| anyhow::anyhow!(e))?;
    let ipv4_broadcast = default_iface.ipv4.pop().map(|ip| ip.broadcast().into());
    let ipv6_broadcast = default_iface.ipv6.pop().map(|ip| ip.broadcast().into());

    ipv4_broadcast
        .or(ipv6_broadcast)
        .ok_or_else(|| anyhow::anyhow!("unable to determine default broadcast address"))
}

async fn broadcast(socket: &UdpSocket, packet: Packet) -> anyhow::Result<()> {
    let buf = packet.encode()?;
    let addr = SocketAddr::new(get_default_broadcast_address()?, LSDP_PORT);

    socket.send_to(&buf, addr).await?;
    Ok(())
}

async fn processor(
    mut query: mpsc::Receiver<QueryMessage>,
    event_bus: EventBus,
    mut cancel: broadcast::Receiver<()>,
) {
    tracing::info!("started device discovery processor");

    'main: loop {
        let Ok(sock) = UdpSocket::bind(format!("0.0.0.0:{LSDP_PORT}")).await else {
            tracing::error!("failed to bind socket");
            sleep(Duration::from_secs(5)).await;
            continue 'main;
        };
        if sock.set_broadcast(true).is_err() {
            tracing::error!("failed to configure socket");
            sleep(Duration::from_secs(5)).await;
            continue 'main;
        }

        let mut buf = [0; 1024];
        let mut devices: HashSet<Device> = HashSet::new();

        loop {
            for stale_device in
                devices.extract_if(|device| device.last_update <= (Utc::now() - STALE_DURATION))
            {
                event_bus.publish_lossy(Event::DeviceGone(stale_device));
            }

            tokio::select! {
                // Handle cancel request
                _ = cancel.recv() => {
                    tracing::debug!("device discovery terminated");
                    break 'main;
                }
                // Handle query requests
                x = query.recv() => match x {
                    Some(message) => {
                        if broadcast(&sock, Packet::new(message)).await.is_err() {
                            tracing::error!("failed to broadcast packet");
                            continue 'main;
                        }
                    },
                    None => {
                        tracing::debug!("query channel terminated");
                        break 'main;
                    }
                },
                // Handle announcements
                x = sock.recv_from(&mut buf) => match x {
                    Ok((size, from)) => match Packet::decode(&buf[..size]) {
                        Ok(packet) => {
                            event_bus.publish_lossy(Event::DiscoveryAnnouncement(
                                from,
                                buf[..size].to_vec(),
                            ));

                            match packet.message {
                                Message::Announce(message) => {
                                    tracing::debug!(?message, "received announce message");

                                    let device = message.into_device(Utc::now());
                                    let network_changed = devices.replace(device.clone()).is_some_and(|d| d.ip_addr != device.ip_addr);

                                    if network_changed {
                                        event_bus.publish_lossy(Event::DeviceGone(device.clone()));
                                    }
                                    event_bus.publish_lossy(Event::DeviceAnnouncement(device));
                                }
                                Message::Delete(message) => {
                                    tracing::debug!(?message, "received delete message");

                                    let device_id = message.device_id();
                                    for mut device in devices.extract_if(|d| d.id == device_id).collect::<Vec<_>>() {
                                        device.attributes.retain(|attr| !message.classes.contains(&attr.class));
                                        device.last_update = Utc::now();
                                        devices.replace(device.clone());
                                        event_bus.publish_lossy(Event::DeviceAnnouncement(device));
                                    }
                                }
                                Message::Query(message) => tracing::debug!(?message, "received query message"),
                            }
                        }
                        Err(error) => tracing::debug!(?error, "malfmored or unknown packet"),
                    }
                    Err(error) => {
                        tracing::warn!(?error, "failed receiving packet");
                        continue 'main;
                    }
                }
            }
        }
    }
}

async fn refresher(
    query: mpsc::Sender<QueryMessage>,
    event_bus: EventBus,
    mut cancel: broadcast::Receiver<()>,
) {
    let mut event_stream = event_bus.subscribe();
    let mut refresh_interval = interval(REFRESH_INTERVAL_SLOW);

    'main: loop {
        // NB: Querying is unreliable, some devices don't respond to the first query.
        for _ in 0..REFRESH_REPEAT_N_TIMES {
            if let Err(error) = query.send(QueryMessage::all_devices()).await {
                tracing::warn!(?error, "failed to refresh devices");
            }

            tokio::select! {
                // Handle cancel request
                _ = cancel.recv() => {
                    tracing::debug!("device refresher terminated");
                    break 'main;
                }
                _ = sleep(REFRESH_REPEAT_INTERVAL) => {}
            }
        }

        loop {
            tokio::select! {
                // Handle cancel request
                _ = cancel.recv() => {
                    tracing::debug!("device refresher terminated");
                    break 'main;
                }
                // Handle events
                event = event_stream.recv() => match event {
                    Ok(Event::ProfileTransitionStarted) => {
                        refresh_interval = interval(REFRESH_INTERVAL_FAST);
                    }
                    Ok(Event::ProfileTransitionCompleted { .. }) => {
                        refresh_interval = interval(REFRESH_INTERVAL_SLOW);
                    }
                    Ok(_) => {}
                    Err(error) => {
                        tracing::error!(?error, "event stream error");
                        continue 'main;
                    }
                },
                _ = refresh_interval.tick() => break,
            }
        }
    }
}

/// A service that monitors the local network for BluOS compatible devices
/// using the LSDP protocol.
#[must_use]
pub struct DeviceDiscovery {
    query: mpsc::Sender<QueryMessage>,
    #[allow(unused)]
    cancel: broadcast::Sender<()>,
}

impl DeviceDiscovery {
    pub async fn start(event_bus: EventBus) -> anyhow::Result<Self> {
        let (query_tx, query_rx) = mpsc::channel(64);
        let (cancel, _) = broadcast::channel(1);
        let this = Self {
            query: query_tx.clone(),
            cancel: cancel.clone(),
        };

        tokio::spawn(processor(query_rx, event_bus.clone(), cancel.subscribe()));
        tokio::spawn(refresher(query_tx, event_bus.clone(), cancel.subscribe()));

        Ok(this)
    }

    pub async fn refresh(&self, classes: &[DeviceClass]) -> anyhow::Result<()> {
        self.query.send(QueryMessage::devices(classes)?).await?;
        Ok(())
    }

    pub async fn refresh_all(&self) -> anyhow::Result<()> {
        self.query.send(QueryMessage::all_devices()).await?;
        Ok(())
    }
}
