mod client;
mod control;
pub mod profile;
mod protocol;

pub use control::DeviceController;
pub use profile::ProfileController;
pub use protocol::{
    AudioPreset, DeviceAudioSettings, DeviceDiagnostics, DeviceGroupStatus, DeviceInputSelection,
    DeviceInputSelectionItem, DevicePlaybackState, DevicePlayerSettings, DevicePreset,
    DevicePresets, DeviceStatus, DeviceVolume, LedBrightness, SettingState, ZoneChannel,
};

pub const VOLUME_LEVEL_MAX: u8 = 100;
pub const VOLUME_LEVEL_MIN: u8 = 0;

// NB: In meters
pub const DISTANCE_MAX: f64 = 10.0;
pub const DISTANCE_MIN: f64 = 0.0;
pub const DISTANCE_STEP_RESOLUTION: f64 = 0.1;
