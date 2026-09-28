use anyhow::Context;
use itertools::Itertools;
use strum::IntoEnumIterator;

use super::prelude::*;
use crate::bluos::profile::{
    GroupProfile, GroupProfileDevice, MultiplayerGroupProfile, MultiplayerGroupProfileSlave,
    Profile,
};
use crate::bluos::{AudioPreset, LedBrightness, SettingState};
use crate::profile::validate_profile_name;
use crate::terminal::app::DeviceState;
use crate::types::DeviceId;

const DB_STEP_RESOLUTION: f64 = 0.5;
const VOLUME_LEVEL_RESOLUTION: f64 = 1.0;

fn cycle_or_unset<T, I>(current: Option<T>, values: I) -> Option<T>
where
    T: PartialEq + Copy,
    I: IntoIterator<Item = T>,
{
    let mut iter = values.into_iter();
    match current {
        None => iter.next(),
        Some(current) => {
            iter.find(|value| *value == current);
            iter.next()
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, strum_macros::EnumIter)]
enum ProfileType {
    SingleDevice,
    #[default]
    NormalGroup,
    MultiplayerGroup,
    HomeTheaterGroup,
}

impl ProfileType {
    fn list_text(&self) -> &'static str {
        match self {
            Self::SingleDevice => "Single Device",
            Self::NormalGroup => "Normal Group",
            Self::MultiplayerGroup => "Multi Player Group",
            Self::HomeTheaterGroup => "Home-theater Group",
        }
    }

    fn description_text(&self) -> &'static str {
        match self {
            Self::NormalGroup => {
                "A standard group of BluOS players that can be controlled individually."
            }
            Self::MultiplayerGroup => "Multiple BluOS players playing the same audio in sync.",
            Self::SingleDevice => "A single BluOS player.",
            Self::HomeTheaterGroup => {
                "Multiple BluOS players in a home-theater surround sound setup."
            }
        }
    }

    fn min_num_devices(&self) -> usize {
        match self {
            Self::SingleDevice => 1,
            Self::NormalGroup => 2,
            Self::MultiplayerGroup => 2,
            Self::HomeTheaterGroup => 3,
        }
    }

    fn max_num_devices(&self) -> usize {
        match self {
            Self::SingleDevice => 1,
            Self::NormalGroup => usize::MAX,
            Self::MultiplayerGroup => usize::MAX,
            Self::HomeTheaterGroup => 3,
        }
    }

    fn is_group(&self) -> bool {
        matches!(
            self,
            Self::NormalGroup | Self::MultiplayerGroup | Self::HomeTheaterGroup
        )
    }
    fn is_zone_group(&self) -> bool {
        matches!(self, Self::MultiplayerGroup | Self::HomeTheaterGroup)
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum WizardStep {
    #[default]
    ChooseProfileType,
    SelectDevices,
    EditDeviceSettings,
    ChooseProfileName,
}

impl WizardStep {
    fn header(&self) -> &'static str {
        match self {
            Self::ChooseProfileType => "choose a profile type",
            Self::SelectDevices => "select devices",
            Self::EditDeviceSettings => "edit device settings",
            Self::ChooseProfileName => "choose a profile name",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeviceRole {
    Master,
    Slave,
    None,
}

#[derive(Debug, Clone)]
struct DeviceSettings {
    device_id: DeviceId,
    volume_level: Option<u8>,
    led_brightness: Option<LedBrightness>,
    node_name: Option<String>,
    audio_preset: Option<AudioPreset>,
    treble_eq: Option<f64>,
    bass_eq: Option<f64>,
    center_trim: Option<f64>,
    volume_trim: Option<f64>,
    surround_upmixer: Option<SettingState>,
    volume_leveler: Option<SettingState>,
    virtualizer: Option<SettingState>,
}

impl DeviceSettings {
    fn new(device_id: DeviceId) -> Self {
        Self {
            device_id,
            volume_level: None,
            led_brightness: None,
            node_name: None,
            audio_preset: None,
            treble_eq: None,
            bass_eq: None,
            center_trim: None,
            volume_trim: None,
            surround_upmixer: None,
            volume_leveler: None,
            virtualizer: None,
        }
    }

    fn is_set(&self, setting: &DeviceSetting) -> bool {
        match setting.kind() {
            DeviceSettingKind::VolumeLevel => self.volume_level.is_some(),
            DeviceSettingKind::LedBrightness => self.led_brightness.is_some(),
            DeviceSettingKind::AudioPreset => self.audio_preset.is_some(),
            DeviceSettingKind::VolumeTrim => self.volume_trim.is_some(),
            DeviceSettingKind::TrebleEq => self.treble_eq.is_some(),
            DeviceSettingKind::BassEq => self.bass_eq.is_some(),
            DeviceSettingKind::CenterTrim => self.center_trim.is_some(),
            DeviceSettingKind::NodeName => self.node_name.is_some(),
            DeviceSettingKind::SurroundUpmixer => self.surround_upmixer.is_some(),
            DeviceSettingKind::VolumeLeveler => self.volume_leveler.is_some(),
            DeviceSettingKind::Virtualizer => self.virtualizer.is_some(),
        }
    }

    fn adjust(&mut self, setting: &DeviceSetting, step: i8) {
        let kind = setting.kind();
        let (current, step_resolution) = match kind {
            DeviceSettingKind::TrebleEq => (self.treble_eq.unwrap_or_default(), DB_STEP_RESOLUTION),
            DeviceSettingKind::BassEq => (self.bass_eq.unwrap_or_default(), DB_STEP_RESOLUTION),
            DeviceSettingKind::CenterTrim => {
                (self.center_trim.unwrap_or_default(), DB_STEP_RESOLUTION)
            }
            DeviceSettingKind::VolumeTrim => {
                (self.volume_trim.unwrap_or_default(), DB_STEP_RESOLUTION)
            }
            DeviceSettingKind::VolumeLevel => (
                self.volume_level.unwrap_or_default() as f64,
                VOLUME_LEVEL_RESOLUTION,
            ),
            _ => return,
        };
        let delta = step as f64 * step_resolution;
        let new = current + delta;
        // TODO: these values should come from the settings of the BluOS device itself
        let clamped = match kind {
            DeviceSettingKind::TrebleEq
            | DeviceSettingKind::BassEq
            | DeviceSettingKind::CenterTrim => new.clamp(-6.0, 6.0),
            DeviceSettingKind::VolumeTrim => new.clamp(-10.0, 10.0),
            DeviceSettingKind::VolumeLevel => new.clamp(0.0, 100.0),
            _ => unreachable!(),
        };
        match kind {
            DeviceSettingKind::TrebleEq => self.treble_eq = Some(clamped),
            DeviceSettingKind::BassEq => self.bass_eq = Some(clamped),
            DeviceSettingKind::CenterTrim => self.center_trim = Some(clamped),
            DeviceSettingKind::VolumeTrim => self.volume_trim = Some(clamped),
            DeviceSettingKind::VolumeLevel => self.volume_level = Some(clamped as u8),
            _ => unreachable!(),
        }
    }

    fn toggle_set(&mut self, setting: &DeviceSetting) {
        match setting.kind() {
            DeviceSettingKind::VolumeLevel => {
                self.volume_level = self.volume_level.map(|_| None).unwrap_or(Some(0));
            }
            DeviceSettingKind::LedBrightness => {
                self.led_brightness = cycle_or_unset(self.led_brightness, LedBrightness::iter())
            }
            DeviceSettingKind::AudioPreset => {
                self.audio_preset = cycle_or_unset(self.audio_preset, AudioPreset::iter())
            }
            DeviceSettingKind::VolumeTrim => {
                self.volume_trim = self.volume_trim.map(|_| None).unwrap_or_default()
            }
            DeviceSettingKind::SurroundUpmixer => {
                self.surround_upmixer = cycle_or_unset(self.surround_upmixer, SettingState::iter());
            }
            DeviceSettingKind::VolumeLeveler => {
                self.volume_leveler = cycle_or_unset(self.volume_leveler, SettingState::iter());
            }
            DeviceSettingKind::Virtualizer => {
                self.virtualizer = cycle_or_unset(self.virtualizer, SettingState::iter());
            }
            DeviceSettingKind::TrebleEq => {
                self.treble_eq = self.treble_eq.map(|_| None).unwrap_or(Some(0.0));
            }
            DeviceSettingKind::BassEq => {
                self.bass_eq = self.bass_eq.map(|_| None).unwrap_or(Some(0.0))
            }
            DeviceSettingKind::CenterTrim => {
                self.center_trim = self.center_trim.map(|_| None).unwrap_or(Some(0.0));
            }
            _ => {}
        }
    }

    fn value_text(&self, setting: &DeviceSetting, stylesheet: &Stylesheet) -> Span<'_> {
        let unset_text = if setting.is_required() {
            "unset (required)".fg(stylesheet.error_color)
        } else {
            "unset".fg(stylesheet.text_color)
        };

        match setting.kind() {
            DeviceSettingKind::VolumeLevel => match self.volume_level {
                Some(v) => format!("{v}").fg(stylesheet.text_color),
                None => unset_text,
            },
            DeviceSettingKind::LedBrightness => match self.led_brightness {
                Some(v) => v.to_string().fg(stylesheet.text_color),
                None => unset_text,
            },
            DeviceSettingKind::NodeName => match self.node_name.as_deref() {
                Some(s) if !s.is_empty() => s.fg(stylesheet.text_color),
                _ => unset_text,
            },
            DeviceSettingKind::AudioPreset => match self.audio_preset {
                Some(v) => v.to_string().fg(stylesheet.text_color),
                None => unset_text,
            },
            DeviceSettingKind::VolumeTrim => match self.volume_trim {
                Some(v) => format!("{v:.1} dB").fg(stylesheet.text_color),
                None => unset_text,
            },
            DeviceSettingKind::TrebleEq => match self.treble_eq {
                Some(v) => format!("{v:.1} dB").fg(stylesheet.text_color),
                None => unset_text,
            },
            DeviceSettingKind::BassEq => match self.bass_eq {
                Some(v) => format!("{v:.1} dB").fg(stylesheet.text_color),
                None => unset_text,
            },
            DeviceSettingKind::CenterTrim => match self.center_trim {
                Some(v) => format!("{v:.1} dB").fg(stylesheet.text_color),
                None => unset_text,
            },
            DeviceSettingKind::SurroundUpmixer => match self.surround_upmixer {
                Some(v) => v.to_string().fg(stylesheet.text_color),
                None => unset_text,
            },
            DeviceSettingKind::VolumeLeveler => match self.volume_leveler {
                Some(v) => v.to_string().fg(stylesheet.text_color),
                None => unset_text,
            },
            DeviceSettingKind::Virtualizer => match self.virtualizer {
                Some(v) => v.to_string().fg(stylesheet.text_color),
                None => unset_text,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeviceSetting {
    Required(DeviceSettingKind),
    Optional(DeviceSettingKind),
}

impl DeviceSetting {
    fn is_available(self, state: &DeviceState) -> bool {
        match self.kind() {
            DeviceSettingKind::LedBrightness => state
                .player_settings
                .as_ref()
                .is_some_and(|s| s.led_brightness.is_some()),
            DeviceSettingKind::AudioPreset => state
                .audio_settings
                .as_ref()
                .is_some_and(|s| s.audio_preset.is_some()),
            DeviceSettingKind::TrebleEq => state
                .audio_settings
                .as_ref()
                .is_some_and(|s| s.equalizer_treble_db.is_some()),
            DeviceSettingKind::BassEq => state
                .audio_settings
                .as_ref()
                .is_some_and(|s| s.equalizer_bass_db.is_some()),
            DeviceSettingKind::CenterTrim => state
                .audio_settings
                .as_ref()
                .is_some_and(|s| s.equalizer_center_trim_db.is_some()),
            DeviceSettingKind::SurroundUpmixer => state
                .audio_settings
                .as_ref()
                .is_some_and(|s| s.surround_upmixer.is_some()),
            DeviceSettingKind::VolumeLeveler => state
                .audio_settings
                .as_ref()
                .is_some_and(|s| s.volume_leveler.is_some()),
            DeviceSettingKind::Virtualizer => state
                .audio_settings
                .as_ref()
                .is_some_and(|s| s.virtualizer.is_some()),
            _ => true,
        }
    }

    fn is_required(&self) -> bool {
        matches!(self, Self::Required(_))
    }

    fn kind(&self) -> DeviceSettingKind {
        match self {
            Self::Required(kind) => *kind,
            Self::Optional(kind) => *kind,
        }
    }

    fn label(&self) -> &'static str {
        match self.kind() {
            DeviceSettingKind::VolumeLevel => "Volume level",
            DeviceSettingKind::LedBrightness => "LED brightness",
            DeviceSettingKind::NodeName => "Device name",
            DeviceSettingKind::AudioPreset => "Audio preset",
            DeviceSettingKind::TrebleEq => "Treble EQ",
            DeviceSettingKind::BassEq => "Bass EQ",
            DeviceSettingKind::CenterTrim => "Center trim",
            DeviceSettingKind::VolumeTrim => "Volume trim",
            DeviceSettingKind::SurroundUpmixer => "Surround upmixer",
            DeviceSettingKind::VolumeLeveler => "Volume leveler",
            DeviceSettingKind::Virtualizer => "Virtualizer",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeviceSettingKind {
    VolumeLevel,
    LedBrightness,
    NodeName,
    AudioPreset,
    TrebleEq,
    BassEq,
    CenterTrim,
    VolumeTrim,
    SurroundUpmixer,
    VolumeLeveler,
    Virtualizer,
}

impl DeviceSettingKind {
    const fn optional(self) -> DeviceSetting {
        DeviceSetting::Optional(self)
    }

    const fn required(self) -> DeviceSetting {
        DeviceSetting::Required(self)
    }
}

#[derive(Debug, Default)]
pub struct NewProfileDialog {
    step: WizardStep,
    selected_profile_type: ProfileType,
    /// If the selected profile type is a (zone) group then the first
    /// entry is the group master, the remaining entries are slaves
    selected_devices_for_group: Vec<DeviceSettings>,
    profile_name_input: TextFieldState,
    focused_device: Option<DeviceId>,
    focused_settings_row: usize,
    inline_text_input: Option<TextFieldState>,
}

impl NewProfileDialog {
    pub fn new() -> Self {
        Self::default()
    }

    fn device_role(&self, device_id: &DeviceId) -> DeviceRole {
        match self
            .selected_devices_for_group
            .iter()
            .position(|d| d.device_id == *device_id)
        {
            Some(0) if self.selected_profile_type.is_group() => DeviceRole::Master,
            Some(_) => DeviceRole::Slave,
            None => DeviceRole::None,
        }
    }

    fn device_settings(&self, device_id: &DeviceId, state: &AppState) -> Vec<DeviceSetting> {
        let profile_settings = match self.selected_profile_type {
            ProfileType::MultiplayerGroup | ProfileType::HomeTheaterGroup
                if self
                    .selected_devices_for_group
                    .first()
                    .is_some_and(|d| d.device_id == *device_id) =>
            {
                vec![
                    DeviceSettingKind::VolumeLevel.optional(),
                    DeviceSettingKind::LedBrightness.optional(),
                    DeviceSettingKind::NodeName.optional(),
                    DeviceSettingKind::AudioPreset.optional(),
                    DeviceSettingKind::TrebleEq.optional(),
                    DeviceSettingKind::BassEq.optional(),
                    DeviceSettingKind::CenterTrim.optional(),
                    DeviceSettingKind::SurroundUpmixer.optional(),
                    DeviceSettingKind::VolumeLeveler.optional(),
                    DeviceSettingKind::Virtualizer.optional(),
                ]
            }
            ProfileType::MultiplayerGroup | ProfileType::HomeTheaterGroup => vec![
                DeviceSettingKind::NodeName.required(),
                DeviceSettingKind::VolumeTrim.optional(),
                DeviceSettingKind::LedBrightness.optional(),
            ],
            ProfileType::SingleDevice | ProfileType::NormalGroup => vec![
                DeviceSettingKind::VolumeLevel.optional(),
                DeviceSettingKind::LedBrightness.optional(),
                DeviceSettingKind::NodeName.optional(),
                DeviceSettingKind::AudioPreset.optional(),
                DeviceSettingKind::TrebleEq.optional(),
                DeviceSettingKind::BassEq.optional(),
                DeviceSettingKind::CenterTrim.optional(),
                DeviceSettingKind::SurroundUpmixer.optional(),
                DeviceSettingKind::VolumeLeveler.optional(),
                DeviceSettingKind::Virtualizer.optional(),
            ],
        };

        profile_settings
            .into_iter()
            .filter(|s| {
                state
                    .find_device(device_id)
                    .is_none_or(|state| s.is_available(state))
            })
            .collect()
    }

    fn adjust_focused_device_setting(&mut self, step: i8, state: &AppState) {
        let Some(device_id) = self.focused_device else {
            return;
        };
        let Some(setting) = self
            .device_settings(&device_id, state)
            .get(self.focused_settings_row)
            .copied()
        else {
            return;
        };
        let Some(device) = self
            .selected_devices_for_group
            .iter_mut()
            .find(|d| d.device_id == device_id)
        else {
            return;
        };

        device.adjust(&setting, step);
    }

    fn selection_block_reason(&self, state: &AppState) -> Option<String> {
        if self.selected_profile_type.is_group() {
            if self.selected_devices_for_group.is_empty() {
                return Some("select a single master device".to_owned());
            }

            if self.selected_devices_for_group.len() < self.selected_profile_type.min_num_devices()
            {
                return Some(format!(
                    "select min required amount of slaves: {}",
                    self.selected_profile_type
                        .min_num_devices()
                        .saturating_sub(1),
                ));
            }
        } else {
            if self.selected_devices_for_group.len() < self.selected_profile_type.min_num_devices()
            {
                return Some(format!(
                    "select min required amount of devices: {}",
                    self.selected_profile_type.min_num_devices(),
                ));
            }
        }

        if self.selected_profile_type.is_zone_group() {
            if !self
                .selected_devices_for_group
                .first()
                .and_then(|d| state.find_device(&d.device_id))
                .and_then(|d| d.group_status.as_ref())
                .and_then(|s| s.zone_options.as_ref())
                .is_some_and(|o| o.is_master_capable())
            {
                return Some("selected master device is not master capable".to_owned());
            }

            if !self.selected_devices_for_group.iter().skip(1).all(|d| {
                state
                    .find_device(&d.device_id)
                    .and_then(|d| d.group_status.as_ref())
                    .and_then(|s| s.zone_options.as_ref())
                    .is_some_and(|o| o.is_slave_capable())
            }) {
                return Some("all slaves must be slave-capable devices".to_owned());
            }
        }

        None
    }

    fn edit_block_reason(&self, state: &AppState) -> Option<String> {
        let device_count = self.selected_devices_for_group.len();
        if device_count == 0 {
            return None;
        }

        let focused_index = self
            .selected_devices_for_group
            .iter()
            .position(|d| self.focused_device.is_some_and(|id| d.device_id == id))
            .unwrap_or_default();

        for offset in 0..device_count {
            let device_index = (focused_index + offset) % device_count;
            let device = self.selected_devices_for_group.get(device_index)?;

            if let Some(setting) = self
                .device_settings(&device.device_id, state)
                .iter()
                .find(|s| s.is_required() && !device.is_set(s))
            {
                let reason = if device_index == focused_index {
                    format!("missing required setting '{}'", setting.label())
                } else {
                    format!(
                        "missing required setting '{}' for device {}",
                        setting.label(),
                        device_index + 1
                    )
                };
                return Some(reason);
            }
        }

        None
    }

    fn build_profile(&self) -> anyhow::Result<Profile> {
        let profile = match self.selected_profile_type {
            ProfileType::NormalGroup => {
                let mut devices: Vec<_> = self
                    .selected_devices_for_group
                    .iter()
                    .map(|d| GroupProfileDevice {
                        device_id: d.device_id,
                        volume_level: d.volume_level,
                        led_brightness: d.led_brightness,
                        node_name: d.node_name.clone(),
                        audio_preset: d.audio_preset,
                        treble_eq: d.treble_eq,
                        bass_eq: d.bass_eq,
                        center_trim: d.center_trim,
                        surround_upmixer: d.surround_upmixer,
                        virtualizer: d.virtualizer,
                        volume_leveler: d.volume_leveler,
                    })
                    .collect();

                anyhow::ensure!(!devices.is_empty(), "no devices");

                Profile::Group(GroupProfile {
                    master: devices.remove(0),
                    slaves: devices,
                    // TODO
                    source: None,
                    ungroup_extra: None,
                })
            }
            ProfileType::MultiplayerGroup => {
                let master = self
                    .selected_devices_for_group
                    .first()
                    .context("no master")?;
                let slaves = self
                    .selected_devices_for_group
                    .iter()
                    .skip(1)
                    .map(|d| {
                        anyhow::Ok(MultiplayerGroupProfileSlave {
                            device_id: d.device_id,
                            led_brightness: d.led_brightness,
                            node_name: d.node_name.clone().context("missing node name")?,
                            volume_trim: d.volume_trim,
                        })
                    })
                    .try_collect()?;

                Profile::MultiplayerGroup(MultiplayerGroupProfile {
                    slaves,
                    master: master.device_id,
                    volume_level: master.volume_level,
                    node_name: master.node_name.clone(),
                    audio_preset: master.audio_preset,
                    treble_eq: master.treble_eq,
                    bass_eq: master.bass_eq,
                    center_trim: master.center_trim,
                    surround_upmixer: master.surround_upmixer,
                    virtualizer: master.virtualizer,
                    volume_leveler: master.volume_leveler,
                    led_brightness: master.led_brightness,
                    // TODO
                    group_name: None,
                    source: None,
                    ungroup_extra: None,
                })
            }
            _ => {
                todo!();
                // let mut devices = self.selected_devices_for_group.iter().enumerate();
                // let (master_index, master) = devices.next()?;
                // let master_settings = settings.get(master_index)?;
                // let slaves = devices
                //     .filter_map(|(index, id)| {
                //         let s = settings.get(index)?;
                //         let node_name = s.node_name.as_deref()?.trim();
                //         if node_name.is_empty() {
                //             return None;
                //         }
                //         Some(MultiplayerGroupProfileSlave {
                //             device_id: *id,
                //             node_name: node_name.to_string(),
                //             volume_trim: s.volume_trim,
                //             led_brightness: s.led_brightness,
                //         })
                //     })
                //     .collect::<Vec<_>>();
                // if slaves.is_empty() {
                //     return None;
                // }

                // Some(Profile::MultiplayerGroup(MultiplayerGroupProfile {
                //     master: *master,
                //     volume_level: master_settings.volume_level,
                //     node_name: master_settings
                //         .node_name
                //         .as_deref()
                //         .filter(|n| !n.trim().is_empty())
                //         .map(str::to_owned),
                //     audio_preset: master_settings.audio_preset,
                //     treble_eq: master_settings.treble_eq,
                //     bass_eq: master_settings.bass_eq,
                //     center_trim: master_settings.center_trim,
                //     surround_upmixer: master_settings.surround_upmixer,
                //     led_brightness: master_settings.led_brightness,
                //     source: None,
                //     group_name: None,
                //     slaves,
                //     ungroup_extra: None,
                // }))
            }
        };

        profile.validate()?;
        Ok(profile)
    }

    fn render_choose_profile_type(&self, area: Rect, ctx: &mut ComponentContext<'_>) {
        let [body, footer] = area.layout(
            &Layout::new(
                Direction::Vertical,
                [Constraint::Fill(1), Constraint::Length(1)],
            )
            .margin(1),
        );
        let [list_area, description_area] = body.layout(
            &Layout::new(
                Direction::Vertical,
                [
                    Constraint::Length(ProfileType::iter().len() as u16),
                    Constraint::Fill(1),
                ],
            )
            .spacing(1)
            .margin(1),
        );

        let mut selected = None;
        let list = ProfileType::iter()
            .enumerate()
            .map(|(index, ty)| {
                if ty == self.selected_profile_type {
                    selected = Some(index);
                }

                ty.list_text().fg(ctx.stylesheet.text_color)
            })
            .collect::<List>()
            .highlight_symbol("> ".fg(ctx.stylesheet.highlight_color).bold())
            .highlight_style(Style::new().bg(ctx.stylesheet.accent_color_dark).bold());
        StatefulWidget::render(
            list,
            list_area,
            ctx.buffer,
            &mut ListState::default().with_selected(selected),
        );

        let mut description = Text::default();
        description.push_line(
            Line::from(self.selected_profile_type.description_text())
                .fg(ctx.stylesheet.text_color_sub),
        );
        let description = Paragraph::new(description)
            .wrap(Wrap { trim: false })
            .alignment(Alignment::Center);
        description.render(description_area, ctx.buffer);

        Keybindings::new(&[("🡳/🡱", "Selection"), ("ESC", "Close"), ("ENTER", "Next")])
            .render(footer, ctx);
    }

    fn render_select_devices(&self, area: Rect, ctx: &mut ComponentContext<'_>) {
        let [body, footer] = area.layout(
            &Layout::new(
                Direction::Vertical,
                [Constraint::Fill(1), Constraint::Length(1)],
            )
            .margin(1),
        );

        let devices: Vec<_> = ctx
            .state
            .sorted_device_state_iter()
            .map(|(id, state)| (*id, state))
            .collect();

        if devices.is_empty() {
            let area = body.centered(Constraint::Length(30), Constraint::Length(1));
            let text = Paragraph::new(
                Line::from("Detecting devices... ⏳".fg(ctx.stylesheet.accent_color))
                    .alignment(Alignment::Center),
            );
            text.render(area, ctx.buffer);
            Keybindings::new(&[("u", "Ungroup all devices"), ("ESC", "Close")]).render(footer, ctx);
        } else {
            let [list_area, hint_area] = body.layout(
                &Layout::new(
                    Direction::Vertical,
                    [
                        Constraint::Length(devices.len().min(10) as u16),
                        Constraint::Fill(1),
                    ],
                )
                .spacing(1)
                .margin(1),
            );

            let mut selected = None;
            let list = devices
                .iter()
                .enumerate()
                .map(|(index, (device_id, state))| {
                    if self.focused_device.is_some_and(|d| d == *device_id) {
                        selected = Some(index);
                    }

                    let DeviceState {
                        device,
                        group_status,
                        ..
                    } = &state;
                    let device_name = state.device_name().unwrap_or_else(|| device.id.to_string());
                    let device_model = state.device_model().unwrap_or("N/A".to_string());

                    let mut line = vec![match self.device_role(device_id) {
                        DeviceRole::Master => "MASTER ".fg(ctx.stylesheet.highlight_color).bold(),
                        DeviceRole::Slave => "☑ ".fg(ctx.stylesheet.text_color).bold(),
                        DeviceRole::None => "☐ ".fg(ctx.stylesheet.text_color_sub).bold(),
                    }];
                    line.push(device_name.fg(ctx.stylesheet.text_color));
                    line.push(format!(" ({device_model})").fg(ctx.stylesheet.text_color_sub));
                    if self.selected_profile_type.is_zone_group() {
                        let (is_master_capable, is_slave_capable) = group_status
                            .as_ref()
                            .and_then(|s| s.zone_options.as_ref())
                            .map(|o| (o.is_master_capable(), o.is_slave_capable()))
                            .unwrap_or_default();
                        let capabilities = match (is_master_capable, is_slave_capable) {
                            (false, false) => String::new(),
                            (false, true) => " [S]".to_string(),
                            (true, false) => " [M]".to_string(),
                            (true, true) => " [M/S]".to_string(),
                        };
                        line.push(capabilities.fg(ctx.stylesheet.highlight_color).bold());
                    }
                    if group_status.as_ref().is_some_and(|s| s.am_i_grouped()) {
                        line.push(" — ".fg(ctx.stylesheet.text_color_sub));
                        line.push("currently grouped".fg(ctx.stylesheet.error_color).bold());
                    }

                    Line::from(line)
                })
                .collect::<List>()
                .highlight_symbol("> ".fg(ctx.stylesheet.highlight_color).bold())
                .highlight_style(Style::new().bg(ctx.stylesheet.accent_color_dark).bold());
            StatefulWidget::render(
                list,
                list_area,
                ctx.buffer,
                &mut ListState::default().with_selected(selected),
            );

            let (hint, keybindings) = if let Some(reason) = self.selection_block_reason(ctx.state) {
                (
                    Line::from(reason)
                        .fg(ctx.stylesheet.error_color)
                        .bold()
                        .alignment(Alignment::Center),
                    Keybindings::new(&[
                        ("🡳/🡱", "Selection"),
                        ("SPACE", "Select device"),
                        ("u", "Ungroup all devices"),
                        ("ESC", "Close"),
                    ]),
                )
            } else {
                (
                    Line::from("selection OK")
                        .fg(ctx.stylesheet.success_color)
                        .bold()
                        .alignment(Alignment::Center),
                    Keybindings::new(&[
                        ("🡳/🡱", "Selection"),
                        ("SPACE", "Select device"),
                        ("u", "Ungroup all devices"),
                        ("ESC", "Close"),
                        ("ENTER", "Next"),
                    ]),
                )
            };
            hint.render(hint_area, ctx.buffer);
            keybindings.render(footer, ctx);
        }
    }

    fn render_edit_device_settings(&self, area: Rect, ctx: &mut ComponentContext<'_>) {
        let [body, footer] = area.layout(
            &Layout::new(
                Direction::Vertical,
                [Constraint::Fill(1), Constraint::Length(1)],
            )
            .margin(1),
        );
        let [header_area, list_area, input_or_hint_area] = body.layout(
            &Layout::new(
                Direction::Vertical,
                [
                    Constraint::Length(1),
                    Constraint::Fill(1),
                    Constraint::Length(3),
                ],
            )
            .spacing(1),
        );

        let device_count = self.selected_devices_for_group.len();
        let device_index = self
            .selected_devices_for_group
            .iter()
            .position(|d| {
                self.focused_device
                    .is_some_and(|device_id| d.device_id == device_id)
            })
            .unwrap_or_default();
        let device = self.selected_devices_for_group.get(device_index);
        let device_name = device
            .and_then(|d| ctx.state.find_device(&d.device_id))
            .map_or_else(String::new, |s| {
                s.device_name().unwrap_or_else(|| s.device.id.to_string())
            });
        let header = Line::from(vec![
            format!("device {} of {device_count}: ", device_index + 1)
                .fg(ctx.stylesheet.text_color_sub),
            device_name.fg(ctx.stylesheet.text_color).bold(),
        ]);
        header.render(header_area, ctx.buffer);

        let list = self
            .focused_device
            .iter()
            .flat_map(|device_id| self.device_settings(device_id, ctx.state))
            .map(|setting| {
                let value = device
                    .map(|d| d.value_text(&setting, ctx.stylesheet))
                    .unwrap_or_else(|| "N/A".fg(ctx.stylesheet.text_color));
                Line::from(vec![
                    format!("{:<20}", setting.label()).fg(ctx.stylesheet.text_color_sub),
                    value,
                ])
            })
            .collect::<List>()
            .highlight_symbol("> ".fg(ctx.stylesheet.highlight_color).bold())
            .highlight_style(Style::new().bg(ctx.stylesheet.accent_color_dark).bold());
        StatefulWidget::render(
            list,
            list_area,
            ctx.buffer,
            &mut ListState::default().with_selected(Some(self.focused_settings_row)),
        );

        if let Some(mut text_input) = self.inline_text_input.clone() {
            TextField::new()
                .block(
                    Block::bordered()
                        .border_type(BorderType::Thick)
                        .fg(ctx.stylesheet.highlight_color),
                )
                .label_style(Style::new().fg(ctx.stylesheet.text_color_sub))
                .text_style(Style::new().fg(ctx.stylesheet.text_color))
                .render(input_or_hint_area, ctx.buffer, &mut text_input);
            Keybindings::new(&[("ESC", "Cancel"), ("ENTER", "Confirm")]).render(footer, ctx);

            return;
        }

        let (hint, keybindings) = if let Some(reason) = self.edit_block_reason(ctx.state) {
            (
                Line::from(reason)
                    .fg(ctx.stylesheet.error_color)
                    .bold()
                    .alignment(Alignment::Center),
                Keybindings::new(&[
                    ("🡳/🡱", "Setting"),
                    ("🡰/🡲", "Adjust"),
                    ("SPACE", "Toggle / Edit"),
                    ("TAB", "Change Device"),
                    ("ESC", "Close"),
                ]),
            )
        } else {
            (
                Line::from("settings OK")
                    .fg(ctx.stylesheet.success_color)
                    .bold()
                    .alignment(Alignment::Center),
                Keybindings::new(&[
                    ("🡳/🡱", "Setting"),
                    ("🡰/🡲", "Adjust"),
                    ("SPACE", "Toggle / Edit"),
                    ("TAB", "Device"),
                    ("ENTER", "Next"),
                    ("ESC", "Close"),
                ]),
            )
        };
        hint.render(input_or_hint_area, ctx.buffer);
        keybindings.render(footer, ctx);
    }

    fn render_choose_profile_name(&self, area: Rect, ctx: &mut ComponentContext<'_>) {
        let [body, footer] = area.layout(&Layout::new(
            Direction::Vertical,
            [Constraint::Fill(1), Constraint::Length(1)],
        ));
        let [input_area, hint_area] = body.layout(
            &Layout::new(
                Direction::Vertical,
                [Constraint::Length(3), Constraint::Length(1)],
            )
            .spacing(1)
            .margin(1),
        );

        let input = TextField::with_label("profile name")
            .block(
                Block::bordered()
                    .border_type(BorderType::Thick)
                    .fg(ctx.stylesheet.highlight_color),
            )
            .label_style(Style::new().fg(ctx.stylesheet.text_color_sub))
            .text_style(Style::new().fg(ctx.stylesheet.text_color));
        input.render(input_area, ctx.buffer, &mut self.profile_name_input.clone());

        let (hint, keybindings) =
            if let Err(error) = validate_profile_name(self.profile_name_input.value()) {
                (
                    Line::from(format!("{error}"))
                        .fg(ctx.stylesheet.error_color)
                        .bold()
                        .alignment(Alignment::Center),
                    Keybindings::new(&[("ESC", "Close")]),
                )
            } else {
                (
                    Line::from("profile name OK")
                        .fg(ctx.stylesheet.success_color)
                        .alignment(Alignment::Center),
                    Keybindings::new(&[("ESC", "Close"), ("ENTER", "Confirm")]),
                )
            };
        hint.render(hint_area, ctx.buffer);
        keybindings.render(footer, ctx);
    }
}

impl DialogComponent for NewProfileDialog {
    fn on_key_press(
        &mut self,
        code: KeyCode,
        modifiers: KeyModifiers,
        state: &AppState,
    ) -> Option<DialogEvent> {
        match (self.step, code) {
            // Choose profile type
            (WizardStep::ChooseProfileType, KeyCode::Enter) => {
                self.step = WizardStep::SelectDevices;
                self.focused_device = state
                    .sorted_device_state_iter()
                    .map(|(device_id, _)| *device_id)
                    .next();
                None
            }
            (WizardStep::ChooseProfileType, KeyCode::Up) => {
                self.selected_profile_type =
                    select_previous(ProfileType::iter(), |t| t, Some(self.selected_profile_type))
                        .unwrap_or_default();
                None
            }
            (WizardStep::ChooseProfileType, KeyCode::Down) => {
                self.selected_profile_type =
                    select_next(ProfileType::iter(), |t| t, Some(self.selected_profile_type))
                        .unwrap_or_default();
                None
            }

            // Select devices
            (WizardStep::SelectDevices, KeyCode::Enter) => {
                if self.selection_block_reason(state).is_none() {
                    self.focused_device =
                        self.selected_devices_for_group.first().map(|d| d.device_id);
                    self.step = WizardStep::EditDeviceSettings;
                }
                None
            }
            (WizardStep::SelectDevices, KeyCode::Up) => {
                self.focused_device = select_previous(
                    state.sorted_device_state_iter(),
                    |(device_id, _)| *device_id,
                    self.focused_device,
                );
                None
            }
            (WizardStep::SelectDevices, KeyCode::Down) => {
                self.focused_device = select_next(
                    state.sorted_device_state_iter(),
                    |(device_id, _)| *device_id,
                    self.focused_device,
                );
                None
            }
            (WizardStep::SelectDevices, KeyCode::Char(' ')) => {
                if let Some(device_id) = self.focused_device {
                    if let Some(index) = self
                        .selected_devices_for_group
                        .iter()
                        .position(|d| d.device_id == device_id)
                    {
                        if self.device_role(&device_id) == DeviceRole::Master {
                            self.selected_devices_for_group.clear();
                        } else {
                            self.selected_devices_for_group.remove(index);
                        }
                    } else if self.selected_devices_for_group.len()
                        < self.selected_profile_type.max_num_devices()
                    {
                        if self.selected_profile_type.is_zone_group() {
                            // For zone groups the first selected device becomes the master, the rest become slaves
                            let (is_master_capable, is_slave_capable) = state
                                .find_device(&device_id)
                                .and_then(|d| d.group_status.as_ref())
                                .and_then(|s| s.zone_options.as_ref())
                                .map(|o| (o.is_master_capable(), o.is_slave_capable()))
                                .unwrap_or_default();

                            if self.selected_devices_for_group.is_empty() {
                                // First device must be master capable
                                if is_master_capable {
                                    self.selected_devices_for_group
                                        .push(DeviceSettings::new(device_id));
                                }
                            } else if is_slave_capable {
                                // Other devices must be slave capable
                                self.selected_devices_for_group
                                    .push(DeviceSettings::new(device_id));
                            }
                        } else {
                            self.selected_devices_for_group
                                .push(DeviceSettings::new(device_id));
                        }
                    }
                }
                None
            }
            (WizardStep::SelectDevices, KeyCode::Char('u') | KeyCode::Char('U')) => {
                Some(DialogEvent::Actions(vec![UserAction::UngroupAll]))
            }

            // Edit devices
            (WizardStep::EditDeviceSettings, KeyCode::Esc) if self.inline_text_input.is_some() => {
                self.inline_text_input = None;
                None
            }
            (WizardStep::EditDeviceSettings, KeyCode::Enter)
                if let Some(text_input) = self
                    .inline_text_input
                    .as_ref()
                    .map(|i| i.value().trim().to_owned()) =>
            {
                if let Some((setting, device)) = self.focused_device.and_then(|device_id| {
                    Some((
                        self.device_settings(&device_id, state)
                            .get(self.focused_settings_row)?
                            .to_owned(),
                        self.selected_devices_for_group
                            .iter_mut()
                            .find(|d| d.device_id == device_id)?,
                    ))
                }) {
                    if setting.kind() == DeviceSettingKind::NodeName {
                        device.node_name = if text_input.is_empty() {
                            None
                        } else {
                            Some(text_input)
                        }
                    }
                }
                self.inline_text_input = None;
                None
            }
            (WizardStep::EditDeviceSettings, _)
                if let Some(text_input) = self.inline_text_input.as_mut() =>
            {
                text_input.on_key_press(code, modifiers);
                None
            }
            (WizardStep::EditDeviceSettings, KeyCode::Enter) => {
                if self.edit_block_reason(state).is_none() {
                    self.step = WizardStep::ChooseProfileName;
                }
                None
            }
            (WizardStep::EditDeviceSettings, KeyCode::Char(' ')) => {
                if let Some((setting, device)) = self.focused_device.and_then(|device_id| {
                    Some((
                        self.device_settings(&device_id, state)
                            .get(self.focused_settings_row)?
                            .to_owned(),
                        self.selected_devices_for_group
                            .iter_mut()
                            .find(|d| d.device_id == device_id)?,
                    ))
                }) {
                    match setting.kind() {
                        DeviceSettingKind::NodeName => {
                            self.inline_text_input = Some(TextFieldState::with_value(
                                device
                                    .node_name
                                    .clone()
                                    .or_else(|| {
                                        state
                                            .find_device(&device.device_id)
                                            .and_then(|s| s.device_name())
                                    })
                                    .unwrap_or_default(),
                            ));
                        }
                        _ => {
                            device.toggle_set(&setting);
                        }
                    }
                }

                None
            }
            (WizardStep::EditDeviceSettings, KeyCode::Tab) => {
                self.focused_settings_row = 0;
                self.focused_device = select_next(
                    self.selected_devices_for_group.iter(),
                    |d| d.device_id,
                    self.focused_device,
                );
                None
            }
            (WizardStep::EditDeviceSettings, KeyCode::Up) => {
                self.focused_settings_row = self.focused_settings_row.saturating_sub(1);
                None
            }
            (WizardStep::EditDeviceSettings, KeyCode::Down) => {
                self.focused_settings_row = (self.focused_settings_row + 1).min(
                    self.focused_device
                        .map(|device_id| {
                            self.device_settings(&device_id, state)
                                .len()
                                .saturating_sub(1)
                        })
                        .unwrap_or_default(),
                );
                None
            }
            (
                WizardStep::EditDeviceSettings,
                KeyCode::Right | KeyCode::Char('+') | KeyCode::Char('='),
            ) => {
                self.adjust_focused_device_setting(1, state);
                None
            }
            (
                WizardStep::EditDeviceSettings,
                KeyCode::Left | KeyCode::Char('-') | KeyCode::Char('_'),
            ) => {
                self.adjust_focused_device_setting(-1, state);
                None
            }

            // Choose profile name
            (WizardStep::ChooseProfileName, KeyCode::Enter) => {
                if validate_profile_name(self.profile_name_input.value()).is_ok() {
                    let event = match self.build_profile() {
                        Ok(profile) => DialogEvent::Submitted(vec![UserAction::NewProfile(
                            self.profile_name_input.value().to_owned(),
                            profile,
                        )]),
                        Err(error) => DialogEvent::ClosedErr(error),
                    };

                    return Some(event);
                }

                None
            }
            (WizardStep::ChooseProfileName, KeyCode::Esc) => Some(DialogEvent::Closed),
            (WizardStep::ChooseProfileName, code) => {
                self.profile_name_input.on_key_press(code, modifiers);
                None
            }

            (_, KeyCode::Esc) => Some(DialogEvent::Closed),
            _ => None,
        }
    }
}

impl Component for NewProfileDialog {
    fn render(&self, area: Rect, ctx: &mut ComponentContext<'_>) {
        let dialog = Popup::with_title(&format!("Create a new profile - {}", self.step.header()))
            .constraints([Constraint::Ratio(3, 5), Constraint::Ratio(1, 2)])
            .block(
                Block::bordered()
                    .border_type(BorderType::Thick)
                    .bg(ctx.stylesheet.popup_background_color),
            );
        let drawable_area = dialog.drawable_area(area);
        dialog.render(area, ctx.buffer);

        match self.step {
            WizardStep::ChooseProfileType => self.render_choose_profile_type(drawable_area, ctx),
            WizardStep::SelectDevices => self.render_select_devices(drawable_area, ctx),
            WizardStep::EditDeviceSettings => self.render_edit_device_settings(drawable_area, ctx),
            WizardStep::ChooseProfileName => self.render_choose_profile_name(drawable_area, ctx),
        }
    }
}
