use std::cmp::Reverse;
use std::collections::HashMap;
use std::path::PathBuf;

use itertools::Itertools;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::prelude::*;

use super::ui::{Ui, UserAction};
use crate::bluos::{
    DeviceAudioPreset, DeviceController, DeviceDiagnostics, DeviceGroupStatus,
    DeviceInputSelection, DeviceStatus, DeviceVolume, ProfileController,
};
use crate::discover::{Device, DeviceDiscovery};
use crate::editor::open_external_editor;
use crate::event::{Event, EventBus};
use crate::profman::{ProfileStorageManager, StoredProfile, create_profile};
use crate::types::{DeviceId, GroupId, ProfileId};

#[derive(Clone)]
pub struct DeviceState {
    pub device: Device,
    pub status: Option<DeviceStatus>,
    pub volume: Option<DeviceVolume>,
    pub group_status: Option<DeviceGroupStatus>,
    pub diagnostics: Option<DeviceDiagnostics>,
    // NB: only available for specific devices
    pub audio_preset: Option<DeviceAudioPreset>,
    pub input_selection: Option<DeviceInputSelection>,
}

impl From<Device> for DeviceState {
    fn from(device: Device) -> Self {
        Self {
            device,
            status: None,
            volume: None,
            group_status: None,
            diagnostics: None,
            audio_preset: None,
            input_selection: None,
        }
    }
}

#[derive(Default)]
pub struct AppState {
    pub device_state: HashMap<DeviceId, DeviceState>,
    pub profiles: HashMap<ProfileId, StoredProfile>,
    pub is_profile_transitioning: bool,
    #[cfg(feature = "ui-enable-logs")]
    pub logs: std::collections::VecDeque<String>,
}

impl AppState {
    pub fn sorted_device_state_iter(&self) -> std::vec::IntoIter<(&DeviceId, &DeviceState)> {
        self.device_state.iter().sorted_by_key(|(id, device)| {
            (
                Reverse(device.group_status.as_ref().map(|g| g.group_id())),
                Reverse(device.group_status.as_ref().map(|g| g.am_i_master())),
                **id,
            )
        })
    }

    pub fn sorted_profiles_iter(&self) -> std::vec::IntoIter<(&ProfileId, &StoredProfile)> {
        self.profiles
            .iter()
            .sorted_by_key(|(_, profile)| &profile.filepath)
    }

    pub fn find_master_of_slave(&self, device_id: DeviceId) -> Option<DeviceId> {
        let group_id = self
            .find_device(&device_id)?
            .group_status
            .as_ref()?
            .group_id()?;
        self.find_master_in_group(group_id)
    }

    pub fn find_master_in_group(&self, group_id: GroupId) -> Option<DeviceId> {
        self.device_state
            .iter()
            .filter(|(_, s)| {
                s.group_status
                    .as_ref()
                    .and_then(|s| s.group_id())
                    .is_some_and(|id| id == group_id)
            })
            .find(|(_, s)| s.group_status.as_ref().is_some_and(|s| s.am_i_master()))
            .map(|(id, _)| id.to_owned())
    }

    pub fn find_profile(&self, profile_id: &ProfileId) -> Option<&StoredProfile> {
        self.profiles.get(profile_id)
    }

    pub fn find_device(&self, device_id: &DeviceId) -> Option<&DeviceState> {
        self.device_state.get(device_id)
    }

    pub fn is_device_playing(&self, device_id: &DeviceId) -> bool {
        self.device_state
            .get(device_id)
            .and_then(|s| s.status.as_ref())
            .is_some_and(|s| s.state.is_playing())
    }
}

#[allow(unused)]
pub struct App {
    pub device_discovery: DeviceDiscovery,
    pub device_controller: DeviceController,
    pub profile_controller: ProfileController,
    pub profile_storage_manager: ProfileStorageManager,
    pub state: AppState,
    pub ui: Ui,
}

impl App {
    pub async fn new(event_bus: EventBus) -> anyhow::Result<Self> {
        let device_discovery = DeviceDiscovery::start(event_bus.clone()).await?;
        let device_controller = DeviceController::start(event_bus.clone()).await?;
        let profile_controller = ProfileController::start(event_bus.clone()).await?;
        let profile_storage_manager = ProfileStorageManager::start(event_bus.clone()).await?;

        Ok(Self {
            device_discovery,
            device_controller,
            profile_controller,
            profile_storage_manager,
            state: Default::default(),
            ui: Default::default(),
        })
    }

    #[tracing::instrument(skip(self))]
    pub async fn handle_app_event(&mut self, event: Event) -> anyhow::Result<()> {
        match &event {
            #[cfg(feature = "ui-enable-logs")]
            Event::Logs(..) => { /* ignore traces */ }
            _ => tracing::debug!("handling app event"),
        };

        match event {
            Event::DeviceAnnouncement(device) => {
                self.device_controller.poll(&device).await?;
                self.state
                    .device_state
                    .entry(device.id)
                    .or_insert_with(|| device.into())
                    .device = device.clone();
            }
            Event::DeviceGone(device) => {
                self.state.device_state.remove(&device.id);
            }
            Event::DeviceStatusUpdated(id, value) => {
                if let Some(state) = self.state.device_state.get_mut(&id) {
                    state.status = Some(value);
                }
            }
            Event::DeviceVolumeUpdated(id, value) => {
                if let Some(state) = self.state.device_state.get_mut(&id) {
                    state.volume = Some(value);
                }
            }
            Event::DeviceGroupStatusUpdated(id, value) => {
                if let Some(state) = self.state.device_state.get_mut(&id) {
                    state.group_status = Some(value);
                }
            }
            Event::DeviceDiagnosticsUpdated(id, value) => {
                if let Some(state) = self.state.device_state.get_mut(&id) {
                    state.diagnostics = Some(value);
                }
            }
            Event::DeviceAudioPresetUpdated(id, value) => {
                if let Some(state) = self.state.device_state.get_mut(&id) {
                    state.audio_preset = Some(value);
                }
            }
            Event::DeviceInputSelectionUpdated(id, value) => {
                if let Some(state) = self.state.device_state.get_mut(&id) {
                    state.input_selection = Some(value);
                }
            }
            Event::ProfilesLoaded(profiles) => {
                self.state.profiles = profiles
                    .into_iter()
                    .map(|profile| (profile.id(), profile))
                    .collect();
            }
            Event::ProfileTransitionStarted => {
                self.state.is_profile_transitioning = true;
            }
            Event::ProfileTransitionCompleted(result) => {
                self.state.is_profile_transitioning = false;

                if let Err(error) = result.as_ref() {
                    tracing::error!(?error, "failed to apply profile");
                    self.ui.show_notification(format!("{error:?}"));
                }
            }
            Event::DiscoveryAnnouncement(..) => {}
            #[cfg(feature = "ui-enable-logs")]
            Event::Logs(logs) => {
                const N: usize = 64;
                logs.into_iter()
                    .take(N)
                    .for_each(|log| self.state.logs.push_front(log));
                self.state.logs.truncate(N);
            }
        }

        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub async fn handle_user_action(&mut self, action: UserAction) -> anyhow::Result<()> {
        tracing::debug!("handling user action");

        match action {
            UserAction::RefreshDevices => {
                self.device_discovery.refresh_all().await?;
                for (device_id, _) in self.state.device_state.iter() {
                    self.device_controller.poll(*device_id).await?;
                }
            }
            UserAction::DeviceVolumeUp(device_id) => {
                self.device_controller.volume_up(device_id, false).await?;
            }
            UserAction::DeviceVolumeDown(device_id) => {
                self.device_controller.volume_down(device_id, false).await?;
            }
            UserAction::GroupVolumeUp(group_id) => {
                if let Some(device_id) = self.state.find_master_in_group(group_id) {
                    self.device_controller.volume_up(device_id, true).await?;
                }
            }
            UserAction::GroupVolumeDown(group_id) => {
                if let Some(device_id) = self.state.find_master_in_group(group_id) {
                    self.device_controller.volume_down(device_id, true).await?;
                }
            }
            UserAction::TogglePausePlay(device_id) => {
                if self.state.is_device_playing(&device_id) {
                    self.device_controller.pause(device_id).await?;
                } else {
                    self.device_controller.play(device_id).await?;
                }
            }
            UserAction::Skip(device_id) => {
                self.device_controller
                    .skip(
                        self.state
                            .find_master_of_slave(device_id)
                            .unwrap_or(device_id),
                    )
                    .await?;
            }
            UserAction::Back(device_id) => {
                self.device_controller
                    .back(
                        self.state
                            .find_master_of_slave(device_id)
                            .unwrap_or(device_id),
                    )
                    .await?;
            }
            UserAction::Mute(device_id) => {
                self.device_controller.mute(device_id, true).await?;
            }
            UserAction::Unmute(device_id) => {
                self.device_controller.mute(device_id, false).await?;
            }
            UserAction::ApplyProfile(profile_id) => {
                if let Some(p) = self.state.find_profile(&profile_id)
                    && let Ok(profile) = p.profile.to_owned()
                {
                    self.profile_controller.apply_profile(profile).await?;
                }
            }
            UserAction::EditProfile(profile_id) => {
                if let Some(p) = self.state.find_profile(&profile_id)
                    && let Err(error) = open_external_editor(&p.filepath)
                {
                    tracing::error!(?error, "failed to open external editor");
                    self.ui
                        .show_notification(format!("{:?}", anyhow::anyhow!(error)));
                }
            }
            UserAction::NewProfile(profile_name) => {
                if let Ok(path) = create_profile(profile_name.as_str()).await
                    && let Err(error) = open_external_editor(path)
                {
                    tracing::error!(?error, "failed to open external editor");
                    self.ui
                        .show_notification(format!("{:?}", anyhow::anyhow!(error)));
                }
            }
            UserAction::DeleteProfile(profile) => {
                if let Err(error) = tokio::fs::remove_file(&profile.filepath).await {
                    tracing::error!(
                        ?error,
                        filepath = %profile.filepath.to_string_lossy(),
                        "failed to remove profile"
                    );
                    self.ui
                        .show_notification(format!("{:?}", anyhow::anyhow!(error)));
                }
            }
        };

        Ok(())
    }
}

pub fn app_dir() -> PathBuf {
    #[allow(deprecated)]
    std::env::home_dir()
        .expect("failed to read homedir?")
        .join(std::env::var("APP_DIR").unwrap_or("blutui".to_owned()))
}

pub fn profiles_dir() -> PathBuf {
    app_dir().join("profiles")
}

#[cfg(feature = "fs-enable-logs")]
pub fn logs_dir() -> PathBuf {
    app_dir().join("logs")
}

#[must_use]
#[allow(unused)]
pub struct LogGuard(Option<WorkerGuard>);

pub fn app_init_logging(event_bus: EventBus) -> LogGuard {
    #[cfg(feature = "fs-enable-logs")]
    let (file_layer_guard, file_layer) = {
        let file_appender = tracing_appender::rolling::daily(logs_dir(), "rolling.log");
        let (tracing_writer, worker_guard) = tracing_appender::non_blocking(file_appender);
        (
            LogGuard(Some(worker_guard)),
            tracing_subscriber::fmt::layer()
                .pretty()
                .with_thread_ids(false)
                .with_file(false)
                .with_line_number(false)
                .with_writer(tracing_writer),
        )
    };

    #[cfg(feature = "ui-enable-logs")]
    let event_layer = {
        use std::io::{ErrorKind, Result, Write};
        use std::mem::take;
        use std::time::Instant;
        use tokio::sync::mpsc;

        use crate::event::Event;

        struct ChannelWriter(mpsc::Sender<String>);

        impl Write for ChannelWriter {
            fn write(&mut self, buffer: &[u8]) -> Result<usize> {
                let message = str::from_utf8(buffer)
                    .map_err(|_| ErrorKind::InvalidData)?
                    .to_owned();

                match self.0.try_send(message) {
                    Ok(_) => Ok(buffer.len()),
                    Err(e) => match e {
                        mpsc::error::TrySendError::Closed(_) => Err(ErrorKind::UnexpectedEof)?,
                        mpsc::error::TrySendError::Full(_) => Err(ErrorKind::WouldBlock)?,
                    },
                }
            }

            fn flush(&mut self) -> Result<()> {
                Ok(())
            }
        }

        let (tx, mut rx) = mpsc::channel(8);
        tokio::spawn(async move {
            let mut logs: Vec<String> = Vec::new();
            let mut sent_at = Instant::now();

            loop {
                use std::time::Duration;

                if !logs.is_empty() && sent_at.elapsed().as_millis() >= 100 {
                    event_bus.publish_lossy(Event::Logs(take(&mut logs)));
                    sent_at = Instant::now();
                }

                if let Ok(log) = tokio::time::timeout(Duration::from_secs(1), rx.recv()).await {
                    match log {
                        Some(log) => logs.push(log),
                        None => break,
                    }
                }
            }
        });

        Some(
            tracing_subscriber::fmt::layer()
                .pretty()
                .with_thread_ids(false)
                .with_file(false)
                .with_line_number(false)
                .with_writer(move || ChannelWriter(tx.clone())),
        )
    };

    let registry =
        tracing_subscriber::registry().with(tracing_subscriber::EnvFilter::from_default_env());

    #[cfg(all(feature = "fs-enable-logs", feature = "ui-enable-logs"))]
    {
        registry.with(file_layer).with(event_layer).init();
        file_layer_guard
    }
    #[cfg(all(feature = "fs-enable-logs", not(feature = "ui-enable-logs")))]
    {
        registry.with(file_layer).init();
        file_layer_guard
    }
    #[cfg(all(not(feature = "fs-enable-logs"), feature = "ui-enable-logs"))]
    {
        registry.with(event_layer).init();
        LogGuard(None)
    }
    #[cfg(all(not(feature = "fs-enable-logs"), not(feature = "ui-enable-logs")))]
    {
        // NB: silence unused warning
        drop(registry);
        drop(event_bus);
        LogGuard(None)
    }
}

pub fn app_init_dir_structure() -> anyhow::Result<()> {
    std::fs::create_dir_all(app_dir())?;
    std::fs::create_dir_all(profiles_dir())?;
    #[cfg(feature = "fs-enable-logs")]
    std::fs::create_dir_all(logs_dir())?;

    Ok(())
}
