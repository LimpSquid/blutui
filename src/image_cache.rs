use std::collections::HashMap;
use std::io::Cursor;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use image::{DynamicImage, ImageReader};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use uuid::{Uuid, uuid};

use crate::event::{Event, EventBus};

const IMAGE_ID_NS: Uuid = uuid!("3652b155-58c6-49d2-b045-ffcaac0e9f08");
const CACHE_SIZE_LIMIT: usize = 32 * 1024 * 1024; // 32 MiB

async fn fetch_file(url: &str, client: Client) -> anyhow::Result<Vec<u8>> {
    tracing::debug!(%url, "fetching file");

    Ok(client
        .get(url)
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?
        .to_vec())
}

#[derive(Clone)]
struct CacheEntry {
    image: Arc<DynamicImage>,
    image_size: usize,
    cached_at: Instant,
}

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(transparent)]
pub struct ImageId(Uuid);

impl ImageId {
    fn new(url: &str) -> Self {
        Self(Uuid::new_v5(&IMAGE_ID_NS, url.as_bytes()))
    }
}

impl std::str::FromStr for ImageId {
    type Err = uuid::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::from_str(s)?))
    }
}

impl std::fmt::Display for ImageId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl From<Uuid> for ImageId {
    fn from(uuid: Uuid) -> Self {
        Self(uuid)
    }
}

impl From<ImageId> for Uuid {
    fn from(image_id: ImageId) -> Self {
        image_id.0
    }
}

#[derive(Clone)]
pub struct ImageCache {
    event_bus: EventBus,
    client: Client,
    // `None` means the image is already being fetched
    cache: Arc<RwLock<HashMap<ImageId, Option<CacheEntry>>>>,
}

#[derive(Clone)]
pub struct Image {
    pub id: ImageId,
    pub image: Arc<DynamicImage>,
}

pub enum GetResult {
    Cached(Image),
    Fetching(ImageId),
}

impl GetResult {
    pub fn image_id(&self) -> ImageId {
        match self {
            Self::Cached(image) => image.id,
            Self::Fetching(id) => *id,
        }
    }

    pub fn image(&self) -> Option<Image> {
        match self {
            Self::Cached(image) => Some(image.clone()),
            Self::Fetching(_) => None,
        }
    }
}

impl ImageCache {
    pub fn new(event_bus: EventBus) -> anyhow::Result<Self> {
        let client = Client::builder().timeout(Duration::from_secs(30)).build()?;

        Ok(Self {
            event_bus,
            client,
            cache: Default::default(),
        })
    }

    pub fn get(&self, url: &str) -> GetResult {
        let image_id = ImageId::new(url);

        // Fetch image
        let Some(cache_entry) = self
            .cache
            .read()
            .expect("poisened lock")
            .get(&image_id)
            .map(|cache_entry| cache_entry.to_owned())
        else {
            let should_fetch = {
                let mut cache = self.cache.write().expect("poisoned lock");
                if cache.contains_key(&image_id) {
                    false
                } else {
                    cache.insert(image_id, None);
                    true
                }
            };
            if should_fetch {
                let client = self.client.clone();
                let event_bus = self.event_bus.clone();
                let url = url.to_owned();
                let cache = self.cache.clone();

                // We assume this task always run until completion unless the application is being stopped
                tokio::spawn(async move {
                    let Ok(image_data) = fetch_file(&url, client).await else {
                        tracing::warn!(%url, "failed to fetch image file");
                        cache.write().expect("poisened lock").remove(&image_id);
                        return;
                    };

                    let worker_cache = cache.clone();
                    let worker = tokio::task::spawn_blocking(move || {
                        let image_size = image_data.len();
                        let Ok(image_reader) =
                            ImageReader::new(Cursor::new(image_data)).with_guessed_format()
                        else {
                            tracing::warn!(%url, "failed to guess image format");
                            worker_cache
                                .write()
                                .expect("poisened lock")
                                .remove(&image_id);
                            return;
                        };
                        let Ok(image) = image_reader.decode() else {
                            tracing::warn!(%url, "failed to decode image");
                            worker_cache
                                .write()
                                .expect("poisened lock")
                                .remove(&image_id);
                            return;
                        };

                        let image = Arc::new(image);
                        let cache_entry = CacheEntry {
                            image_size,
                            image: image.clone(),
                            cached_at: Instant::now(),
                        };

                        {
                            let mut cache = worker_cache.write().expect("poisened lock");
                            // Calculate the current size once.
                            let current_size = cache.values().fold(0usize, |total, entry| {
                                total.saturating_add(
                                    entry.as_ref().map_or(0, |entry| entry.image_size),
                                )
                            });
                            let replaced_size = cache
                                .get(&image_id)
                                .and_then(Option::as_ref)
                                .map_or(0, |entry| entry.image_size);
                            let mut projected_size = current_size
                                .saturating_sub(replaced_size)
                                .saturating_add(image_size);

                            cache.remove(&image_id);

                            if image_size <= CACHE_SIZE_LIMIT {
                                let mut eviction_candidates: Vec<_> = cache
                                    .iter()
                                    .filter_map(|(image_id, entry)| {
                                        entry.as_ref().map(|entry| {
                                            (*image_id, entry.cached_at, entry.image_size)
                                        })
                                    })
                                    .collect();
                                eviction_candidates
                                    .sort_unstable_by_key(|(_, cached_at, _)| *cached_at);

                                for (image_id, _, size) in eviction_candidates {
                                    if projected_size <= CACHE_SIZE_LIMIT {
                                        break;
                                    }

                                    // This should succeed becauses candidates only contain completed cache entries
                                    assert!(cache.remove(&image_id).is_some());
                                    projected_size = projected_size.saturating_sub(size);
                                }

                                cache.insert(image_id, Some(cache_entry));
                            }
                        }

                        let event = Image {
                            id: image_id,
                            image,
                        };
                        event_bus.publish_lossy(Event::ImageFetched(event.into()));
                    });

                    if let Err(error) = worker.await {
                        tracing::error!(%error, "image cache worker failed");
                        cache.write().expect("poisened lock").remove(&image_id);
                    }
                });
            }

            return GetResult::Fetching(image_id);
        };

        match cache_entry {
            Some(cache_entry) => GetResult::Cached(Image {
                id: image_id,
                image: cache_entry.image.clone(),
            }),
            None => GetResult::Fetching(image_id),
        }
    }
}
