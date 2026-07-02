use std::{sync::Arc, time::Duration};

use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, mpsc};

use super::super::client::HttpClient;
use super::common::SharedClientMap;
use super::device_profile::DeviceProfile;
use super::group_profile::GroupProfile;
use super::multiplayer_group_profile::MultiplayerGroupProfile;
use crate::event::{Event, EventBus};

#[derive(Debug, Clone, Hash, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(untagged)]
pub enum Profile {
    Device(DeviceProfile),
    Group(GroupProfile),
    MultiplayerGroup(MultiplayerGroupProfile),
}

impl Profile {
    pub fn validate(&self) -> anyhow::Result<()> {
        match self {
            Self::Device(p) => p.validate()?,
            Self::Group(p) => p.validate()?,
            Self::MultiplayerGroup(p) => p.validate()?,
        }

        Ok(())
    }

    #[tracing::instrument(err, skip_all)]
    async fn apply(self, clients: SharedClientMap) -> anyhow::Result<()> {
        match self {
            Self::Device(p) => p.apply(clients).await,
            Self::Group(p) => p.apply(clients).await,
            Self::MultiplayerGroup(p) => p.apply(clients).await,
        }
    }
}

#[tracing::instrument(skip_all)]
async fn queue_processor(
    mut queue: mpsc::Receiver<Profile>,
    event_bus: EventBus,
    clients: SharedClientMap,
    mut cancel: broadcast::Receiver<()>,
) {
    loop {
        tokio::select! {
            // Handle cancel request
            _ = cancel.recv() => {
                tracing::debug!("profile queue processor terminated");
                break;
            },
            job = queue.recv() => match job  {
                None => {
                    tracing::debug!("queue channel terminated");
                    break;
                }
                Some(profile) => {
                    event_bus.publish_lossy(Event::ProfileTransitionStarted);
                    let result = match tokio::time::timeout(
                        Duration::from_secs(120),
                        profile.apply(clients.clone())
                    ).await {
                        Ok(result) => result,
                        Err(_) => Err(anyhow::anyhow!("failed to apply profile in time")),
                    };
                    event_bus.publish_lossy(Event::ProfileTransitionCompleted(Arc::new(result)));
                }
            }
        }
    }
}

#[tracing::instrument(skip_all)]
async fn event_processor(
    event_bus: EventBus,
    clients: SharedClientMap,
    mut cancel: broadcast::Receiver<()>,
) {
    'main: loop {
        let mut event_stream = event_bus.subscribe();

        loop {
            tokio::select! {
                // Handle cancel request
                _ = cancel.recv() => {
                    tracing::debug!("profile event processor terminated");
                    break 'main;
                },
                // Handle events
                event = event_stream.recv() => match event {
                    Ok(Event::DeviceAnnouncement(device)) => {
                        // Create the HTTP client
                        clients
                            .write()
                            .await
                            .entry(device.id)
                            .or_insert_with(|| HttpClient::from_device(&device));
                    }
                    Ok(Event::DeviceGone(device)) => {
                        // Remove the HTTP client
                        clients.write().await.remove(&device.id);
                    }
                    Ok(_) => {}
                    Err(error) => {
                        tracing::error!(?error, "event stream error");
                        continue 'main;
                    }
                }
            };
        }
    }
}

/// A service that controls one or more BluOS compatible to apply a user-defined profile
#[must_use]
pub struct ProfileController {
    queue: mpsc::Sender<Profile>,
    #[allow(unused)]
    cancel: broadcast::Sender<()>,
}

impl ProfileController {
    pub async fn start(event_bus: EventBus) -> anyhow::Result<Self> {
        let (queue_tx, queue_rx) = mpsc::channel(64);
        let (cancel, _) = broadcast::channel(1);
        let this = Self {
            queue: queue_tx,
            cancel: cancel.clone(),
        };
        let clients = SharedClientMap::default();

        tokio::spawn(event_processor(
            event_bus.clone(),
            clients.clone(),
            cancel.subscribe(),
        ));
        tokio::spawn(queue_processor(
            queue_rx,
            event_bus.clone(),
            clients.clone(),
            cancel.subscribe(),
        ));

        Ok(this)
    }

    pub async fn apply_profile(&self, profile: Profile) -> anyhow::Result<()> {
        self.queue.send(profile).await?;

        Ok(())
    }
}
