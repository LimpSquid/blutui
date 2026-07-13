use std::net::SocketAddr;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;

use tokio::sync::broadcast;

use crate::bluos::{
    DeviceAudioPreset, DeviceDiagnostics, DeviceGroupStatus, DeviceInputSelection, DeviceStatus,
    DeviceVolume,
};
use crate::discover::Device;
use crate::profman::StoredProfile;
use crate::types::DeviceId;

#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum Event {
    DiscoveryAnnouncement(SocketAddr, Vec<u8>),
    DeviceAnnouncement(Device),
    DeviceGone(Device),
    DeviceStatusUpdated(DeviceId, DeviceStatus),
    DeviceVolumeUpdated(DeviceId, DeviceVolume),
    DeviceGroupStatusUpdated(DeviceId, DeviceGroupStatus),
    DeviceDiagnosticsUpdated(DeviceId, DeviceDiagnostics),
    DeviceAudioPresetUpdated(DeviceId, DeviceAudioPreset),
    DeviceInputSelectionUpdated(DeviceId, DeviceInputSelection),
    DeviceControllerBusy,
    DeviceControllerIdle,
    ProfileTransitionStarted,
    ProfileTransitionCompleted(Arc<anyhow::Result<()>>),
    ProfilesLoaded(Vec<StoredProfile>),
    #[cfg(feature = "ui-enable-logs")]
    Logs(Vec<String>),
}

pub struct EventStream(broadcast::Receiver<Event>);

impl EventStream {
    pub async fn recv_all(&mut self) -> anyhow::Result<Vec<Event>> {
        let mut result = vec![self.recv().await?];

        while let Ok(msg) = self.try_recv() {
            result.push(msg);
        }

        Ok(result)
    }
}

impl Deref for EventStream {
    type Target = broadcast::Receiver<Event>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for EventStream {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

/// A lightweight pub-sub bus for broadcasting [Event]s to asynchronous subscribers
#[derive(Debug, Clone)]
pub struct EventBus {
    pipe: broadcast::Sender<Event>,
}

impl EventBus {
    pub fn new() -> Self {
        let (pipe, _) = broadcast::channel(1024);

        Self { pipe }
    }

    pub fn subscribe(&self) -> EventStream {
        EventStream(self.pipe.subscribe())
    }

    pub fn publish(&self, event: Event) -> anyhow::Result<()> {
        self.pipe.send(event).map_err(|e| anyhow::anyhow!("{e}"))?;

        Ok(())
    }

    pub fn publish_lossy(&self, event: Event) {
        let _ = self.publish(event);
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}
