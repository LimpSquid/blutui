use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use notify::{
    Config, Event as NotifyEvent, EventKind as NotifyEventKind, RecommendedWatcher, RecursiveMode,
    Watcher,
};
use tokio::fs::{DirEntry, File, read_dir, read_to_string};
use tokio::sync::broadcast;
use tokio::time::sleep;

use crate::bluos::profile::Profile;
use crate::event::{Event, EventBus};
use crate::terminal::profiles_dir;
use crate::types::ProfileId;

const EXTENSION: &str = "prof";

fn async_watcher() -> anyhow::Result<(RecommendedWatcher, broadcast::Receiver<NotifyEvent>)> {
    let (tx, rx) = broadcast::channel(1);
    let watcher = RecommendedWatcher::new(
        move |res: Result<NotifyEvent, _>| {
            if let Ok(event) = res
                && matches!(
                    event.kind,
                    NotifyEventKind::Create(..)
                        | NotifyEventKind::Modify(..)
                        | NotifyEventKind::Remove(..)
                )
            {
                tx.send(event).ok();
            }
        },
        Config::default().with_compare_contents(true),
    )?;

    Ok((watcher, rx))
}

#[derive(Debug, Clone)]
pub struct StoredProfile {
    pub filepath: PathBuf,
    pub profile: Result<Profile, String>,
    pub last_modified: SystemTime,
}

impl StoredProfile {
    pub fn id(&self) -> ProfileId {
        ProfileId::new(self.filepath.to_owned())
    }

    pub fn name(&self) -> String {
        self.filepath
            .file_stem()
            .map(|s| s.to_string_lossy())
            .unwrap_or_else(|| self.filepath.to_string_lossy())
            .to_string()
    }
}

async fn profile_from_file(path: impl AsRef<Path>) -> anyhow::Result<Profile> {
    let contents = read_to_string(&path).await?;

    let profile: Profile = if let Ok(value) = yaml_serde::from_str::<yaml_serde::Value>(&contents) {
        yaml_serde::from_value(value)?
    } else if let Ok(value) = serde_json::from_str::<serde_json::Value>(&contents) {
        serde_json::from_value(value)?
    } else {
        anyhow::bail!("profile file is not valid JSON or YAML");
    };

    profile.validate()?;

    Ok(profile)
}

async fn profiles_dir_entries() -> anyhow::Result<Vec<DirEntry>> {
    let mut entries = Vec::new();
    let mut listing = read_dir(profiles_dir()).await?;

    while let Some(entry) = listing.next_entry().await? {
        let path = entry.path();

        if path.extension().and_then(|s| s.to_str()) == Some(EXTENSION) {
            entries.push(entry)
        }
    }

    Ok(entries)
}

#[tracing::instrument(skip_all)]
async fn profile_dir_watcher(
    event_bus: EventBus,
    mut cancel: broadcast::Receiver<()>,
) -> anyhow::Result<()> {
    tracing::info!("started profile dir watcher");

    let mut stored_profiles: HashMap<PathBuf, StoredProfile> = HashMap::new();
    let mut poll_dir = async || -> anyhow::Result<()> {
        let mut updated = false;
        let entries = profiles_dir_entries().await?;

        // Insert or update stored profiles
        for entry in entries.iter() {
            let metadata = entry.metadata().await?;
            let last_modified = metadata.modified()?;

            match stored_profiles.entry(entry.path()) {
                // New profile
                Entry::Vacant(map_entry) => {
                    let filepath = entry.path();
                    let profile = profile_from_file(&filepath)
                        .await
                        .map_err(|e| format!("{e:?}"));
                    map_entry.insert(StoredProfile {
                        filepath,
                        profile,
                        last_modified,
                    });
                    updated = true;
                }
                // Check existing profile for modification
                Entry::Occupied(mut map_entry) => {
                    if last_modified > map_entry.get().last_modified {
                        updated = true;

                        let filepath = entry.path();
                        let profile = profile_from_file(&filepath)
                            .await
                            .map_err(|e| format!("{e:?}"));
                        map_entry.get_mut().profile = profile;
                        map_entry.get_mut().last_modified = last_modified;
                    }
                }
            }
        }

        // Remove stored profile when it got removed from the filesystem
        for path in stored_profiles.keys().cloned().collect::<Vec<_>>() {
            if entries.iter().find(|e| e.path() == path).is_none() {
                stored_profiles.remove(&path);
                updated = true;
            }
        }

        if updated {
            event_bus.publish_lossy(Event::ProfilesLoaded(
                stored_profiles.values().cloned().collect(),
            ));
        }

        Ok(())
    };

    // TODO: handle errors
    let (mut watcher, mut notify) = async_watcher()?;
    watcher
        .watch(&profiles_dir(), RecursiveMode::NonRecursive)
        .unwrap();

    loop {
        if let Err(error) = poll_dir().await {
            tracing::error!(?error, "failed to poll profile dir");
        }

        tokio::select! {
            // Handle cancel request
            _ = cancel.recv() => {
                tracing::debug!("profile dir watcher terminated");
                break;
            },
            // Handle dir changes
            _ = notify.recv() => {}
            // Fallback polling
            _ = sleep(Duration::from_secs(10)) => {}
        }
    }

    Ok(())
}

/// A service that manages the user-defined profiles
#[must_use]
pub struct ProfileManager {
    #[allow(unused)]
    cancel: broadcast::Sender<()>,
}

impl ProfileManager {
    pub async fn start(event_bus: EventBus) -> anyhow::Result<Self> {
        let (cancel, _) = broadcast::channel(1);
        let this = Self {
            cancel: cancel.clone(),
        };

        tokio::spawn(profile_dir_watcher(event_bus, cancel.subscribe()));

        Ok(this)
    }
}

pub async fn create_profile(profile_name: &str) -> anyhow::Result<PathBuf> {
    validate_profile_name(profile_name)?;

    let path = profiles_dir().join(format!("{profile_name}.{EXTENSION}"));
    File::create_new(&path).await?;

    Ok(path)
}

pub fn validate_profile_name(profile_name: &str) -> anyhow::Result<()> {
    anyhow::ensure!(!profile_name.is_empty(), "profile name cannot be empty");
    anyhow::ensure!(
        profile_name.is_ascii(),
        "profile name should only contain ASCII characters"
    );
    anyhow::ensure!(
        !profile_name.contains('/'),
        "profile name cannot contain '/'"
    );
    anyhow::ensure!(
        !profile_name.contains(&format!(".{EXTENSION}")),
        "profile name cannot contain '.{EXTENSION}'",
    );
    anyhow::ensure!(
        !profile_name.chars().all(|c| c.is_whitespace()),
        "profile name cannot only contain whitespace"
    );

    Ok(())
}
