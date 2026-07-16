mod client;
mod control;
pub mod profile;
mod protocol;

pub use control::DeviceController;
pub use profile::ProfileController;
pub use protocol::{
    AudioPreset, DeviceAudioSettings, DeviceDiagnostics, DeviceGroupStatus, DeviceInputSelection,
    DeviceInputSelectionItem, DevicePlayerSettings, DeviceState, DeviceStatus, DeviceVolume,
    LedBrightness,
};

pub const MAX_VOLUME_LEVEL: u8 = 100;
pub const MIN_VOLUME_LEVEL: u8 = 0;
