use std::collections::HashMap;
use std::time::Duration;

use tokio::sync::{broadcast, mpsc};
use tokio::time::sleep;

use super::client::{HttpClient, PollOpts};
use crate::discover::Device;
use crate::event::{Event, EventBus};
use crate::types::DeviceId;

#[derive(Debug, Clone, Copy)]
struct ActionRequest {
    device_id: DeviceId,
    action: Action,
}

#[derive(Debug, Clone, Copy)]
enum Action {
    VolumeStep(f32, bool),
    Pause,
    Play,
    Stop,
    Skip,
    Back,
    Poll,
    Mute(bool),
}

#[tracing::instrument(skip(client))]
async fn handle_action(device_id: DeviceId, client: &HttpClient, action: Action) -> Vec<Event> {
    let result: anyhow::Result<Vec<Event>> = async {
        let events = match action {
            Action::VolumeStep(step, tell_slaves) => {
                client.volume_step(step, tell_slaves).await?;
                vec![]
            }
            Action::Play => {
                client.play(None).await?;
                vec![]
            }
            Action::Pause => {
                client.pause().await?;
                vec![]
            }
            Action::Stop => {
                client.stop().await?;
                vec![]
            }
            Action::Skip => {
                client.skip().await?;
                vec![]
            }
            Action::Back => {
                client.back().await?;
                vec![]
            }
            Action::Mute(on) => {
                client.mute(on).await?;
                vec![]
            }
            Action::Poll => {
                let (diagnostics, input_selection) =
                    tokio::try_join!(client.get_diagnostics(), client.get_input_selection())?;
                vec![
                    Event::DeviceDiagnosticsUpdated(device_id, diagnostics),
                    Event::DeviceInputSelectionUpdated(device_id, input_selection),
                ]
            }
        };

        Ok(events)
    }
    .await;

    match result {
        Ok(events) => events,
        Err(error) => {
            tracing::error!(?error, "failed to handle action");
            vec![]
        }
    }
}

#[tracing::instrument(skip_all)]
async fn processor(
    mut action: mpsc::Receiver<ActionRequest>,
    event_bus: EventBus,
    mut cancel: broadcast::Receiver<()>,
) {
    tracing::info!("started BluOS control processor");

    'main: loop {
        let mut event_stream = event_bus.subscribe();
        let mut syncer_cancel: HashMap<DeviceId, broadcast::Sender<()>> = HashMap::new();
        let mut clients: HashMap<DeviceId, HttpClient> = HashMap::new();
        let mut action_buf = Vec::new();

        loop {
            tokio::select! {
                // Handle cancel request
                _ = cancel.recv() => {
                    tracing::debug!("device controller terminated");
                    break 'main;
                },
                 // Handle action requests
                n_reqs = action.recv_many(&mut action_buf, 128) => match n_reqs {
                    0 => {
                        tracing::debug!("action channel terminated");
                        break 'main;
                    }
                    _ => {
                        event_bus.publish_lossy(Event::DeviceControllerBusy);

                        let tasks_to_poll = action_buf
                            .drain(..)
                            // Action requests are serialized per device but run concurrently across devices
                            .fold(HashMap::new(), |mut acc: HashMap<_, Vec<_>>, req| {
                                acc.entry(req.device_id).or_default().push(req.action);
                                acc
                            })
                            .into_iter()
                            .filter_map(|(device_id, actions)| {
                                Some((device_id, clients.get(&device_id).cloned()?, actions))
                            })
                            .map(|(device_id, client, actions)| async move {
                                let mut events = Vec::new();
                                for action in actions {
                                    events.extend(handle_action(device_id, &client, action).await);
                                }
                                events
                            })
                            .map(tokio::task::spawn);

                        futures::future::try_join_all(tasks_to_poll)
                            .await
                            .ok()
                            .unwrap_or_default()
                            .into_iter()
                            .flatten()
                            .chain(std::iter::once(Event::DeviceControllerIdle))
                            .for_each(|e| event_bus.publish_lossy(e));
                    },
                },
                // Handle device discovery events
                event = event_stream.recv() => match event {
                    Ok(Event::DeviceAnnouncement(device)) => {
                        // Create the HTTP client
                        clients.entry(device.id).or_insert_with(|| {
                            HttpClient::from_device(&device)
                        });

                        // Start device syncer task
                        syncer_cancel.entry(device.id).or_insert_with(|| {
                            let (cancel, _) = broadcast::channel(1);
                            start_device_sync(device, event_bus.clone(), cancel.subscribe());
                            cancel
                        });
                    }
                    Ok(Event::DeviceGone(device)) => {
                        // Stop device syncer task
                        syncer_cancel.remove(&device.id);

                        // Remove the HTTP client
                        clients.remove(&device.id);
                    }
                    Ok(_) => {}
                    Err(error) => {
                        tracing::error!(?error, "event stream error");
                        continue 'main;
                    }
                },
            }
        }
    }
}

#[tracing::instrument(skip_all)]
fn start_device_sync(device: Device, event_bus: EventBus, cancel: broadcast::Receiver<()>) {
    tracing::info!(device_id = %device.id, "started BluOS device syncers");

    let client = HttpClient::from_device(&device);

    tokio::spawn(device_status_syncer(
        device.clone(),
        event_bus.clone(),
        client.clone(),
        cancel.resubscribe(),
    ));
    tokio::spawn(device_volume_syncer(
        device.clone(),
        event_bus.clone(),
        client.clone(),
        cancel.resubscribe(),
    ));
    tokio::spawn(device_group_status_syncer(
        device.clone(),
        event_bus.clone(),
        client.clone(),
        cancel.resubscribe(),
    ));
}

#[tracing::instrument(skip_all)]
async fn device_status_syncer(
    device: Device,
    event_bus: EventBus,
    client: HttpClient,
    mut cancel: broadcast::Receiver<()>,
) {
    let mut poll_opts = None;

    'main: loop {
        tokio::select! {
            // Handle cancel request
            _ = cancel.recv() => {
                tracing::debug!(device_id = %device.id, "device status syncer terminated");
                break 'main;
            }
            // Handle status changes
            status = client.get_device_status(poll_opts.clone()) => match status {
                Ok(status) => {
                    poll_opts = Some(PollOpts::new(&status.etag));
                    event_bus.publish_lossy(Event::DeviceStatusUpdated(device.id, status));
                }
                Err(error) => {
                    tracing::error!(device_id = %device.id, ?error, "failed to sync device status");
                    sleep(Duration::from_secs(1)).await;
                }
            }
        }
    }
}

#[tracing::instrument(skip_all)]
async fn device_volume_syncer(
    device: Device,
    event_bus: EventBus,
    client: HttpClient,
    mut cancel: broadcast::Receiver<()>,
) {
    let mut poll_opts = None;

    'main: loop {
        tokio::select! {
            // Handle cancel request
            _ = cancel.recv() => {
                tracing::debug!(device_id = %device.id, "device status syncer terminated");
                break 'main;
            }
            // Handle volume changes
            volume = client.get_volume(poll_opts.clone()) => match volume {
                Ok(volume) => {
                    poll_opts = Some(PollOpts::new(&volume.etag));
                    event_bus.publish_lossy(Event::DeviceVolumeUpdated(device.id, volume));
                }
                Err(error) => {
                    tracing::error!(device_id = %device.id, ?error, "failed to sync device volume");
                    sleep(Duration::from_secs(1)).await;
                }
            }
        }
    }
}

#[tracing::instrument(skip_all)]
async fn device_group_status_syncer(
    device: Device,
    event_bus: EventBus,
    client: HttpClient,
    mut cancel: broadcast::Receiver<()>,
) {
    let mut poll_opts = None;

    'main: loop {
        tokio::select! {
            // Handle cancel request
            _ = cancel.recv() => {
                tracing::debug!(device_id = %device.id, "device group status syncer terminated");
                break 'main;
            }
            // Handle group status changes
            status = client.get_group_status(poll_opts.clone()) => match status {
                Ok(status) => {
                    let audio_preset_url = status.audio_preset_url.clone();

                    poll_opts = Some(PollOpts::new(&status.etag));
                    event_bus.publish_lossy(Event::DeviceGroupStatusUpdated(device.id, status));

                    if let Some(audio_preset_url) = audio_preset_url {
                        match client.get_audio_preset(&audio_preset_url.url).await {
                            Ok(audio_preset) => {
                                event_bus.publish_lossy(Event::DeviceAudioPresetUpdated(device.id, audio_preset))
                            }
                            Err(error) => {
                                tracing::error!(device_id = %device.id, ?error, "failed to get audio preset");
                            }
                        }
                    }
                }
                Err(error) => {
                    tracing::error!(device_id = %device.id, ?error, "failed to sync device group status");
                    sleep(Duration::from_secs(1)).await;
                }
            }
        }
    }
}

/// A service to control BluOS compatible devices
#[must_use]
pub struct DeviceController {
    action: mpsc::Sender<ActionRequest>,
    #[allow(unused)]
    cancel: broadcast::Sender<()>,
}

impl DeviceController {
    pub async fn start(event_bus: EventBus) -> anyhow::Result<Self> {
        let (action_tx, action_rx) = mpsc::channel(64);
        let (cancel, _) = broadcast::channel(1);
        let this = Self {
            action: action_tx,
            cancel: cancel.clone(),
        };

        tokio::spawn(processor(action_rx, event_bus, cancel.subscribe()));

        Ok(this)
    }

    pub async fn volume_up(
        &self,
        device: impl Into<DeviceId>,
        tell_slaves: bool,
    ) -> anyhow::Result<()> {
        let request = ActionRequest {
            device_id: device.into(),
            action: Action::VolumeStep(2.0, tell_slaves),
        };
        self.action.send(request).await?;

        Ok(())
    }

    pub async fn volume_down(
        &self,
        device: impl Into<DeviceId>,
        tell_slaves: bool,
    ) -> anyhow::Result<()> {
        let request = ActionRequest {
            device_id: device.into(),
            action: Action::VolumeStep(-2.0, tell_slaves),
        };
        self.action.send(request).await?;

        Ok(())
    }

    pub async fn poll(&self, device: impl Into<DeviceId>) -> anyhow::Result<()> {
        let request = ActionRequest {
            device_id: device.into(),
            action: Action::Poll,
        };
        self.action.send(request).await?;

        Ok(())
    }

    pub async fn play(&self, device: impl Into<DeviceId>) -> anyhow::Result<()> {
        let request = ActionRequest {
            device_id: device.into(),
            action: Action::Play,
        };
        self.action.send(request).await?;

        Ok(())
    }

    pub async fn pause(&self, device: impl Into<DeviceId>) -> anyhow::Result<()> {
        let request = ActionRequest {
            device_id: device.into(),
            action: Action::Pause,
        };
        self.action.send(request).await?;

        Ok(())
    }

    pub async fn stop(&self, device: impl Into<DeviceId>) -> anyhow::Result<()> {
        let request = ActionRequest {
            device_id: device.into(),
            action: Action::Stop,
        };
        self.action.send(request).await?;

        Ok(())
    }

    pub async fn skip(&self, device: impl Into<DeviceId>) -> anyhow::Result<()> {
        let request = ActionRequest {
            device_id: device.into(),
            action: Action::Skip,
        };
        self.action.send(request).await?;

        Ok(())
    }

    pub async fn back(&self, device: impl Into<DeviceId>) -> anyhow::Result<()> {
        let request = ActionRequest {
            device_id: device.into(),
            action: Action::Back,
        };
        self.action.send(request).await?;

        Ok(())
    }

    pub async fn mute(&self, device: impl Into<DeviceId>, on: bool) -> anyhow::Result<()> {
        let request = ActionRequest {
            device_id: device.into(),
            action: Action::Mute(on),
        };
        self.action.send(request).await?;

        Ok(())
    }
}
