use std::time::Instant;

use chrono::{Duration, Utc};
use itertools::Itertools;
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Margin, Rect, Spacing};
use ratatui::style::{Style, Styled, Stylize};
use ratatui::symbols::merge::MergeStrategy;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::canvas::Canvas;
use ratatui::widgets::{
    Bar, BarChart, Block, BorderType, Gauge, List, ListState, Paragraph, Scrollbar,
    ScrollbarOrientation, ScrollbarState, Tabs, Wrap,
};
use strum::IntoEnumIterator;

#[allow(unused)]
use super::{
    Ui,
    components::{self, *},
    theme::*,
    utils::*,
    widgets,
};
use crate::bluos::profile::{Profile, SourceSelection};
use crate::bluos::{
    AudioPreset, DevicePlaybackState, LedBrightness, MAX_VOLUME_LEVEL, SettingState,
};
use crate::terminal::app::{AppState, BusyFlags, DeviceState};
use crate::types::DeviceId;

struct RenderContext<'a, 'b> {
    frame: &'a mut Frame<'b>,
    state: &'a AppState,
    ui: &'a mut Ui,
}

// NB: This must be a macro so we can hold a mutable borrow of fields in `RenderContext` that
// do not overlap with fields of components::RenderContext
macro_rules! component_context {
    ($ctx:expr) => {
        ComponentContext {
            buffer: ($ctx).frame.buffer_mut(),
            state: ($ctx).state,
            stylesheet: &($ctx).ui.stylesheet,
        }
    };
}

#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    strum_macros::Display,
    strum_macros::EnumIter,
    strum_macros::EnumCount,
)]
#[strum(serialize_all = "lowercase")]
pub enum WindowFocus {
    Tabs,
    #[default]
    DiscoveredDevices,
}

impl From<usize> for WindowFocus {
    fn from(v: usize) -> Self {
        match v {
            0 => Self::Tabs,
            1 => Self::DiscoveredDevices,
            _ => Self::default(),
        }
    }
}

#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    strum_macros::Display,
    strum_macros::EnumIter,
    strum_macros::EnumCount,
    strum_macros::FromRepr,
)]
#[strum(serialize_all = "lowercase")]
pub enum Tab {
    #[default]
    Profile,
    Music,
    #[cfg(feature = "ui-enable-logs")]
    Logs,
}

impl From<usize> for Tab {
    fn from(v: usize) -> Self {
        Self::from_repr(v).unwrap_or_default()
    }
}

pub fn before_render(state: &AppState, ui: &mut Ui) {
    tracing::trace!("render start");

    if ui
        .selected_device
        .as_ref()
        .is_none_or(|device| state.find_device(device).is_none())
    {
        ui.selected_device = state
            .sorted_device_state_iter()
            .map(|(device_id, _)| *device_id)
            .next();
    }

    if ui
        .selected_profile
        .as_ref()
        .is_none_or(|profile| state.find_profile(profile).is_none())
    {
        ui.selected_profile = state
            .sorted_profiles_iter()
            .map(|(profile_id, _)| profile_id.to_owned())
            .next();
    }

    #[cfg(feature = "ui-enable-image")]
    if let Some(image) = ui
        .selected_device
        .and_then(|device_id| state.get_device_image(&device_id))
    {
        ui.music_image.set_image(image);
    } else {
        ui.music_image.clear_image();
    }

    ui.render_start = Instant::now();
}

pub fn after_render(_: &AppState, ui: &mut Ui) {
    let render_time = ui.render_start.elapsed();

    tracing::trace!(?render_time, "render end");

    if render_time.as_millis() >= 100 {
        tracing::warn!(?render_time, "slow render");
    }
}

pub fn render(frame: &mut Frame, state: &AppState, ui: &mut Ui) {
    let canvas_area = frame.area();
    let canvas = Canvas::default()
        .background_color(ui.stylesheet.background_color)
        .paint(|_| {});
    frame.render_widget(canvas, canvas_area);

    let window_layout = Layout::new(
        Direction::Vertical,
        [Constraint::Fill(1), Constraint::Length(2)],
    )
    .margin(1)
    .spacing(1)
    .split(canvas_area);
    let (body, footer) = (window_layout[0], window_layout[1]);
    let body_layout = Layout::new(
        Direction::Horizontal,
        [Constraint::Percentage(35), Constraint::Fill(1)],
    )
    .spacing(Spacing::Overlap(1))
    .split(body);
    let device_layout = Layout::new(
        Direction::Vertical,
        [Constraint::Percentage(35), Constraint::Fill(1)],
    )
    .spacing(Spacing::Overlap(1))
    .split(body_layout[0]);

    let mut ctx = RenderContext { frame, state, ui };

    render_discovered_devices_window(&mut ctx, device_layout[0]);
    render_device_details_window(&mut ctx, device_layout[1]);
    render_tabs_window(&mut ctx, body_layout[1]);
    render_keybindings(&mut ctx, footer);
    render_busy_indicator(&mut ctx, canvas_area);
    render_dialog(&mut ctx, canvas_area);
}

fn render_busy_indicator(ctx: &mut RenderContext<'_, '_>, area: Rect) {
    if !ctx.state.busy_flags.is_empty() {
        ctx.frame.render_widget(
            "●".fg(ctx.ui.stylesheet.highlight_color),
            Rect::new(area.width.max(1) - 1, area.height.max(1) - 1, 1, 1),
        );
    }
}

#[tracing::instrument(skip_all)]
fn render_dialog(ctx: &mut RenderContext<'_, '_>, area: Rect) {
    if let Some(dialog) = ctx.ui.active_dialogs.front() {
        dialog.render(area, &mut component_context!(ctx));
    }
}

fn render_keybindings(ctx: &mut RenderContext<'_, '_>, area: Rect) {
    let keybindings: Vec<_> = match ctx.ui.window_focus {
        WindowFocus::DiscoveredDevices => vec![
            ("SPACEBAR", "Change Focus"),
            ("🡳/🡱/HOME/END", "Selection"),
            ("r", "Refresh"),
            ("b/n", "Back/Skip"),
            ("(CTRL) + j/l", "(Device) Volume Up/Down"),
            (
                "p",
                if ctx
                    .ui
                    .selected_device
                    .is_some_and(|device_id| ctx.state.is_device_playing(&device_id))
                {
                    "Pause"
                } else {
                    "Play"
                },
            ),
            ("q", "Quit"),
        ],
        WindowFocus::Tabs if ctx.ui.selected_tab == Tab::Profile => vec![
            ("SPACEBAR", "Change Focus"),
            ("🡳/🡱/HOME/END", "Selection"),
            ("n", "New"),
            ("e", "Edit"),
            ("d", "Delete"),
            ("ENTER", "Apply"),
            ("TAB", "Change Tab"),
            ("q", "Quit"),
        ],
        WindowFocus::Tabs if ctx.ui.selected_tab == Tab::Music => vec![
            ("SPACEBAR", "Change Focus"),
            ("🡳/🡱/HOME/END", "Selection"),
            ("b/n", "Back/Skip"),
            ("(CTRL) + j/l", "(Device) Volume Up/Down"),
            (
                "p",
                if ctx
                    .ui
                    .selected_device
                    .is_some_and(|device_id| ctx.state.is_device_playing(&device_id))
                {
                    "Pause"
                } else {
                    "Play"
                },
            ),
            ("TAB", "Change Tab"),
            ("q", "Quit"),
        ],
        WindowFocus::Tabs => vec![
            ("SPACEBAR", "Change Focus"),
            ("TAB", "Change Tab"),
            ("q", "Quit"),
        ],
    }
    .into_iter()
    .map(|(keys, desc)| {
        (
            keys.replace(" ", &format!("{NON_BREAKING_SPACE}")),
            desc.replace(" ", &format!("{NON_BREAKING_SPACE}")),
        )
    })
    .collect();

    Keybindings::new(&keybindings).render(area, &mut component_context!(ctx));
}

fn render_discovered_devices_window(ctx: &mut RenderContext<'_, '_>, area: Rect) {
    let groupbox_area = render_groupbox(
        ctx,
        Some("devices"),
        area,
        ctx.ui.window_focus == WindowFocus::DiscoveredDevices,
    );

    if ctx.state.device_state.is_empty() {
        let paragraph =
            Paragraph::new("Detecting devices... ⏳".fg(ctx.ui.stylesheet.accent_color))
                .wrap(Wrap { trim: false });
        let area = area.centered(
            Constraint::Length(paragraph.line_width().min(area.width as usize) as u16),
            Constraint::Length(paragraph.line_count(area.width) as u16),
        );
        ctx.frame.render_widget(paragraph, area);
    } else {
        let mut selected = None;
        let list = ctx
            .state
            .sorted_device_state_iter()
            .enumerate()
            .map(|(index, (device_id, state))| {
                if ctx.ui.selected_device.is_some_and(|id| id == *device_id) {
                    selected = Some(index);
                }

                let DeviceState {
                    device,
                    group_status,
                    ..
                } = &state;

                let device_name = state.device_name().unwrap_or_else(|| device_id.to_string());
                let device_model = state.device_model().unwrap_or("N/A".to_string());
                let device_last_update = device.last_update;

                vec![Line::from(vec![
                    device_name.fg(ctx.ui.stylesheet.text_color),
                    format!(" ({device_model})").fg(ctx.ui.stylesheet.text_color_sub),
                    if let Some((group_color, group_status)) = group_status
                        .as_ref()
                        .and_then(|s| Some((uuid_to_color(s.group_id()?), s)))
                    {
                        if group_status.am_i_master() {
                            format!(" {MASTER_SYMBOL}")
                        } else if group_status.am_i_zone_slave() {
                            format!(" {ZONE_SLAVE_SYMBOL}")
                        } else if group_status.am_i_slave() {
                            format!(" {SLAVE_SYMBOL}")
                        } else {
                            "".to_string()
                        }
                        .fg(group_color)
                    } else {
                        "".into()
                    },
                    if Utc::now() - device_last_update >= Duration::seconds(90) {
                        Span::from(format!(" {WARNING_SYMBOL}"))
                    } else {
                        Span::from("")
                    },
                ])]
            })
            .collect::<List>()
            .highlight_style(Style::new().bg(ctx.ui.stylesheet.accent_color_dark).bold());

        ctx.frame.render_stateful_widget(
            list,
            groupbox_area,
            &mut ListState::default().with_selected(selected),
        );
    }
}

fn render_device_details_window(ctx: &mut RenderContext<'_, '_>, area: Rect) {
    let groupbox_area = render_groupbox(ctx, Some("device details"), area, false);

    if let Some(DeviceState {
        device,
        volume,
        group_status,
        diagnostics,
        input_selection,
        audio_settings,
        player_settings,
        ..
    }) = ctx
        .ui
        .selected_device
        .and_then(|device_id| ctx.state.find_device(&device_id))
    {
        let mut data = Vec::new();
        data.push(("device id".to_string(), Some(device.id.to_string())));
        match group_status.as_ref().and_then(|s| s.mac_address.as_ref()) {
            Some(mac) => data.push((
                "interface".to_string(),
                Some(format!("{} ({mac})", device.ip_addr)),
            )),
            None => data.push(("connection".to_string(), Some(device.ip_addr.to_string()))),
        }
        if diagnostics
            .as_ref()
            .is_none_or(|d| d.connected_to_network.is_some())
        {
            data.push((
                "connection".to_string(),
                diagnostics.as_ref().map(|d| {
                    match (d.connected_to_network.as_ref(), d.signal_strength.as_ref()) {
                        (Some(ctn), Some(ss)) => format!("{ctn} ({ss})"),
                        (Some(ctn), None) => ctn.to_string(),
                        (_, _) => "N/A".to_string(),
                    }
                }),
            ));
        }
        data.push((
            "uptime".to_string(),
            diagnostics
                .as_ref()
                .map(|d| d.uptime.to_owned().unwrap_or("N/A".to_string())),
        ));
        data.push((
            "volume level".to_string(),
            volume
                .as_ref()
                .map(|v| format!("{} ({} dB)", v.volume, v.db)),
        ));
        if let Some(input_selection) = input_selection
            && !input_selection.item.is_empty()
        {
            data.push((
                "input source(s)".to_string(),
                Some(
                    input_selection
                        .item
                        .iter()
                        .map(|i| i.text.to_lowercase())
                        .join(", "),
                ),
            ));
        }
        if let Some(zone_options) = group_status.as_ref().and_then(|s| s.zone_options.as_ref()) {
            data.push((
                "capabilities".to_string(),
                Some(
                    match (
                        zone_options.is_master_capable(),
                        zone_options.is_slave_capable(),
                    ) {
                        (true, true) => "primary or secondary player",
                        (true, false) => "primary player",
                        (false, true) => "secondary player",
                        (false, false) => "cannot be grouped in a zone",
                    }
                    .to_string(),
                ),
            ));
        }
        if let Some(led_brightness) = player_settings.as_ref().and_then(|p| p.led_brightness) {
            data.push((
                "led indicator".to_string(),
                Some(led_brightness.to_string()),
            ));
        }
        if let Some(audio_preset) = audio_settings.as_ref().and_then(|p| p.audio_preset) {
            data.push(("audio preset".to_string(), Some(audio_preset.to_string())));
        }
        if let Some(db) = audio_settings.as_ref().and_then(|p| p.equalizer_bass_db) {
            data.push(("bass eq".to_string(), Some(format!("{db} dB"))));
        }
        if let Some(db) = audio_settings.as_ref().and_then(|p| p.equalizer_treble_db) {
            data.push(("treble eq".to_string(), Some(format!("{db} dB"))));
        }
        if let Some(db) = audio_settings
            .as_ref()
            .and_then(|p| p.equalizer_center_trim_db)
        {
            data.push(("center trim".to_string(), Some(format!("{db} dB"))));
        }
        if let Some(upmixer) = audio_settings.as_ref().and_then(|p| p.surround_upmixer) {
            data.push(("surround upmix".to_string(), Some(upmixer.to_string())));
        }
        if let Some(volume_leveler) = audio_settings.as_ref().and_then(|p| p.volume_leveler) {
            data.push((
                "volume leveler".to_string(),
                Some(volume_leveler.to_string()),
            ));
        }
        if let Some(virtualizer) = audio_settings.as_ref().and_then(|p| p.virtualizer) {
            data.push(("virtualizer".to_string(), Some(virtualizer.to_string())));
        }

        let data_key_max_len = data
            .iter()
            .map(|(k, _)| k.chars().count())
            .max()
            .unwrap_or_default() as u16;
        let spacing = 1u16;
        let separator = " ";
        let loading = "Loading... ⏳";

        let vertical_layout = Layout::vertical(data.iter().map(|(_, v)| {
            let chars_needed_for_label =
                data_key_max_len + spacing + separator.chars().count() as u16 + spacing;
            let chars_needed_for_value = v.as_deref().unwrap_or(loading).chars().count() as u16;
            let chars_available_per_line =
                groupbox_area.width - chars_needed_for_label.min(groupbox_area.width);
            if chars_available_per_line >= chars_needed_for_value {
                Constraint::Length(1)
            } else if chars_available_per_line == 0 {
                Constraint::Length(0)
            } else {
                Constraint::Length(chars_needed_for_value.div_ceil(chars_available_per_line))
            }
        }))
        .split(groupbox_area);

        for i in 0..data.len() {
            let horizontal_layout = Layout::new(
                Direction::Horizontal,
                [
                    Constraint::Length(data_key_max_len), // key
                    Constraint::Length(1),                // seperator
                    Constraint::Fill(1),                  // value
                ],
            )
            .spacing(spacing)
            .split(vertical_layout[i]);

            let (key, value) = &data[i];
            ctx.frame.render_widget(
                Paragraph::new(key.as_str().fg(ctx.ui.stylesheet.text_color_sub))
                    .wrap(Wrap { trim: false }),
                horizontal_layout[0],
            );
            ctx.frame.render_widget(
                separator.fg(ctx.ui.stylesheet.text_color_sub),
                horizontal_layout[1],
            );
            ctx.frame.render_widget(
                Paragraph::new(match value {
                    Some(value) => value.as_str().fg(ctx.ui.stylesheet.text_color),
                    None => loading.fg(ctx.ui.stylesheet.text_color),
                })
                .wrap(Wrap { trim: false }),
                horizontal_layout[2],
            );
        }
    } else {
        let text = Line::from(
            if ctx.state.device_state.is_empty() {
                "Detecting devices... ⏳"
            } else {
                "Select a device."
            }
            .fg(ctx.ui.stylesheet.accent_color),
        );
        let paragraph = Paragraph::new(text).wrap(Wrap { trim: false });
        let area = area.centered(
            Constraint::Length(paragraph.line_width().min(area.width as usize) as u16),
            Constraint::Length(paragraph.line_count(area.width) as u16),
        );
        ctx.frame.render_widget(paragraph, area);
    }
}

fn render_profile_details_window(ctx: &mut RenderContext<'_, '_>, area: Rect) {
    let selected_profile = ctx
        .ui
        .selected_profile
        .as_ref()
        .and_then(|profile_id| ctx.state.find_profile(profile_id))
        .map(|p| p.profile.as_ref());
    let profile_title = match selected_profile {
        Some(Ok(Profile::Device(_))) => "single device details",
        Some(Ok(Profile::Group(_))) => "normal group details",
        Some(Ok(Profile::MultiplayerGroup(_))) => "multi-player group details",
        _ => "profile details",
    };
    let area = render_groupbox(ctx, Some(profile_title), area, false);

    if ctx
        .state
        .busy_flags
        .contains(BusyFlags::PROFILE_TRANSITIONING)
    {
        let paragraph = Paragraph::new("Applying profile... ⏳".fg(ctx.ui.stylesheet.accent_color))
            .wrap(Wrap { trim: false });
        let area = area.centered(
            Constraint::Length(paragraph.line_width().min(area.width as usize) as u16),
            Constraint::Length(paragraph.line_count(area.width) as u16),
        );
        ctx.frame.render_widget(paragraph, area);
        return;
    }

    let Some(selected_profile) = selected_profile else {
        let paragraph = Paragraph::new("Select a profile.".fg(ctx.ui.stylesheet.accent_color))
            .wrap(Wrap { trim: false });
        let area = area.centered(
            Constraint::Length(paragraph.line_width().min(area.width as usize) as u16),
            Constraint::Length(paragraph.line_count(area.width) as u16),
        );
        ctx.frame.render_widget(paragraph, area);
        return;
    };
    let Ok(selected_profile) = selected_profile else {
        let text = selected_profile
            .unwrap_err()
            .lines()
            .map(|l| Line::from(l.fg(ctx.ui.stylesheet.error_color)))
            .collect::<Vec<_>>();
        let paragraph = Paragraph::new(Text::from(text)).wrap(Wrap { trim: false });
        let area = area.centered(
            Constraint::Length(paragraph.line_width().min(area.width as usize) as u16),
            Constraint::Length(paragraph.line_count(area.width) as u16),
        );
        ctx.frame.render_widget(paragraph, area);
        return;
    };

    struct Row {
        device_id: DeviceId,
        role: Option<&'static str>,
        settings: Vec<(&'static str, Option<String>)>,
    }

    let (main_device_id, source, group_name) = match selected_profile {
        Profile::Device(p) => (Some(p.device_id), p.source.as_ref(), None),
        Profile::Group(p) => (Some(p.master.device_id), p.source.as_ref(), None),
        Profile::MultiplayerGroup(p) => {
            (Some(p.master), p.source.as_ref(), p.group_name.as_deref())
        }
    };

    let device_settings = |volume_level: Option<u8>,
                           volume_trim: Option<f64>,
                           led_brightness: Option<LedBrightness>,
                           node_name: Option<&str>,
                           audio_preset: Option<AudioPreset>,
                           treble_eq: Option<f64>,
                           bass_eq: Option<f64>,
                           center_trim: Option<f64>,
                           surround_upmixer: Option<SettingState>,
                           volume_leveler: Option<SettingState>,
                           virtualizer: Option<SettingState>|
     -> Vec<(&'static str, Option<String>)> {
        let mut settings = Vec::new();
        if let Some(v) = node_name {
            settings.push(("device name", Some(v.to_owned())));
        }
        if let Some(v) = volume_level {
            settings.push(("volume level", Some(v.to_string())));
        }
        if let Some(v) = volume_trim {
            settings.push(("volume trim", Some(format!("{v:.1} dB"))));
        }
        if let Some(v) = led_brightness {
            settings.push(("led brightness", Some(v.to_string())));
        }
        if let Some(v) = audio_preset {
            settings.push(("audio preset", Some(v.to_string())));
        }
        if let Some(v) = treble_eq {
            settings.push(("treble eq", Some(format!("{v:.1} dB"))));
        }
        if let Some(v) = bass_eq {
            settings.push(("bass eq", Some(format!("{v:.1} dB"))));
        }
        if let Some(v) = center_trim {
            settings.push(("center trim", Some(format!("{v:.1} dB"))));
        }
        if let Some(v) = surround_upmixer {
            settings.push(("surround upmix", Some(v.to_string())));
        }
        if let Some(v) = volume_leveler {
            settings.push(("volume leveler", Some(v.to_string())));
        }
        if let Some(v) = virtualizer {
            settings.push(("virtualizer", Some(v.to_string())));
        }

        settings
    };

    let mut rows: Vec<Row> = Vec::new();

    match selected_profile {
        Profile::Device(p) => {
            rows.push(Row {
                role: None,
                device_id: p.device_id,
                settings: device_settings(
                    p.volume_level,
                    None,
                    p.led_brightness,
                    p.node_name.as_deref(),
                    p.audio_preset,
                    p.treble_eq,
                    p.bass_eq,
                    p.center_trim,
                    p.surround_upmixer,
                    p.volume_leveler,
                    p.virtualizer,
                ),
            });
        }
        Profile::Group(p) => {
            rows.push(Row {
                role: Some("master"),
                device_id: p.master.device_id,
                settings: device_settings(
                    p.master.volume_level,
                    None,
                    p.master.led_brightness,
                    p.master.node_name.as_deref(),
                    p.master.audio_preset,
                    p.master.treble_eq,
                    p.master.bass_eq,
                    p.master.center_trim,
                    p.master.surround_upmixer,
                    p.master.volume_leveler,
                    p.master.virtualizer,
                ),
            });
            for slave in p.slaves.iter() {
                rows.push(Row {
                    role: Some("slave"),
                    device_id: slave.device_id,
                    settings: device_settings(
                        slave.volume_level,
                        None,
                        slave.led_brightness,
                        slave.node_name.as_deref(),
                        slave.audio_preset,
                        slave.treble_eq,
                        slave.bass_eq,
                        slave.center_trim,
                        slave.surround_upmixer,
                        slave.volume_leveler,
                        slave.virtualizer,
                    ),
                });
            }
        }
        Profile::MultiplayerGroup(p) => {
            rows.push(Row {
                role: Some("master"),
                device_id: p.master,
                settings: device_settings(
                    p.volume_level,
                    None,
                    p.led_brightness,
                    p.node_name.as_deref(),
                    p.audio_preset,
                    p.treble_eq,
                    p.bass_eq,
                    p.center_trim,
                    p.surround_upmixer,
                    p.volume_leveler,
                    p.virtualizer,
                ),
            });
            for slave in p.slaves.iter() {
                rows.push(Row {
                    role: Some("zone slave"),
                    device_id: slave.device_id,
                    settings: device_settings(
                        None,
                        slave.volume_trim,
                        slave.led_brightness,
                        Some(slave.node_name.as_str()),
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                    ),
                });
            }
        }
    }

    let label_width = rows
        .iter()
        .flat_map(|r| r.settings.iter())
        .map(|(label, _)| label.chars().count())
        .max()
        .unwrap_or_default()
        .saturating_add(2);
    let mut lines = Vec::new();

    if let Some(group_name) = group_name {
        lines.push(Line::from(vec![
            format!("{:<label_width$}", "group name").fg(ctx.ui.stylesheet.text_color_sub),
            group_name.fg(ctx.ui.stylesheet.text_color),
        ]));
    }

    if let Some(source) = source {
        let value = match source {
            SourceSelection::Input { input } => {
                Span::from(format!("input: {input}")).fg(ctx.ui.stylesheet.text_color)
            }
            SourceSelection::Preset { preset_id } => {
                let preset_name = main_device_id
                    .and_then(|device_id| ctx.state.find_device(&device_id))
                    .and_then(|device| device.presets.as_ref())
                    .and_then(|presets| {
                        presets
                            .preset
                            .iter()
                            .find(|preset| *preset.id == *preset_id)
                            .map(|preset| preset.name.clone())
                    });

                match preset_name {
                    Some(name) => Span::from(format!("preset: {name} ({preset_id})"))
                        .fg(ctx.ui.stylesheet.text_color),
                    None => Span::from(format!("preset '{preset_id}' is not available"))
                        .fg(ctx.ui.stylesheet.error_color),
                }
            }
        };
        lines.push(Line::from(vec![
            format!("{:<label_width$}", "source").fg(ctx.ui.stylesheet.text_color_sub),
            value.fg(ctx.ui.stylesheet.text_color),
        ]));
    }

    if !lines.is_empty() {
        lines.push(Line::from(""));
    }

    for (index, row) in rows.into_iter().enumerate() {
        if index > 0 {
            lines.push(Line::from(""));
        }

        let (name, model, status) = match ctx.state.find_device(&row.device_id) {
            Some(device) => (
                device
                    .device_name()
                    .unwrap_or_else(|| row.device_id.to_string()),
                device.device_model().unwrap_or_default(),
                None,
            ),
            None => (
                row.device_id.to_string(),
                String::new(),
                Some("(not available)"),
            ),
        };

        let mut header = Vec::new();
        header.push(name.fg(ctx.ui.stylesheet.text_color).bold());
        if !model.is_empty() {
            header.push(format!(" ({model})").fg(ctx.ui.stylesheet.text_color_sub));
        }
        if let Some(status) = status {
            header.push(format!(" {status}").fg(ctx.ui.stylesheet.text_color_sub));
        }
        if let Some(role) = row.role {
            let role = role
                .to_ascii_uppercase()
                .fg(ctx.ui.stylesheet.accent_color_dark)
                .bold();

            let header_width: usize = header.iter().map(|span| span.width()).sum();
            let padding = (area.width as usize).saturating_sub(header_width + role.width());
            header.push(Span::from(" ".repeat(padding)));
            header.push(role);
        }

        lines.push(Line::from(header));
        lines.push(
            Line::from("─".repeat(area.width as usize)).fg(ctx.ui.stylesheet.border_color_dark),
        );

        for (setting_label, setting_value) in row.settings {
            lines.push(Line::from(vec![
                format!("{setting_label:<label_width$}").fg(ctx.ui.stylesheet.text_color_sub),
                match setting_value {
                    Some(v) => Span::from(v.clone()).fg(ctx.ui.stylesheet.text_color),
                    None => "unset".fg(ctx.ui.stylesheet.text_color_sub),
                },
            ]));
        }
    }

    ctx.frame
        .render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), area);
}

fn render_tabs_window(ctx: &mut RenderContext<'_, '_>, area: Rect) {
    let highlight = ctx.ui.window_focus == WindowFocus::Tabs;
    let layout = Layout::new(
        Direction::Vertical,
        [
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Fill(1),
        ],
    )
    .spacing(Spacing::Overlap(1))
    .split(area);

    let tabs = Tabs::new(Tab::iter().map(|v| v.to_string()))
        .select(ctx.ui.selected_tab as usize)
        .style(Style::default().fg(ctx.ui.stylesheet.text_color))
        .divider(LINE_DIVIDER.fg(ctx.ui.stylesheet.text_color_sub))
        .block({
            let mut block = Block::bordered()
                .border_type(BorderType::Rounded)
                .merge_borders(MergeStrategy::Fuzzy)
                .border_style(Style::default().fg(ctx.ui.stylesheet.border_color));

            if highlight {
                block = block.title(Line::from(vec![
                    format!(" {HIGHLIGHT_SYMBOL} ").fg(ctx.ui.stylesheet.highlight_color),
                ]))
            }
            block
        })
        .highlight_style(
            Style::default()
                .bg(ctx.ui.stylesheet.accent_color_dark)
                .bold(),
        );
    ctx.frame.render_widget(tabs, layout[0]);
    render_groupbox(ctx, None, layout[1], false);

    match ctx.ui.selected_tab {
        Tab::Profile => render_profile_tab(ctx, layout[2]),
        Tab::Music => render_music_tab(ctx, layout[2]),
        #[cfg(feature = "ui-enable-logs")]
        Tab::Logs => render_logs_tab(ctx, layout[2]),
    }
}

fn render_profile_tab(ctx: &mut RenderContext<'_, '_>, area: Rect) {
    let mut selected = None;
    let list = ctx
        .state
        .sorted_profiles_iter()
        .map(|(profile_id, profile)| {
            (
                ctx.ui
                    .selected_profile
                    .as_ref()
                    .is_some_and(|id| id == profile_id),
                profile,
            )
        })
        .enumerate()
        .map(|(index, (is_selected, p))| {
            if is_selected {
                selected = Some(index);
            }

            format!(
                "{}{}",
                p.name(),
                if p.profile.is_err() {
                    format!(" {WARNING_SYMBOL}")
                } else {
                    "".to_string()
                }
            )
            .fg(ctx.ui.stylesheet.text_color)
        })
        .collect::<List>()
        .highlight_style(Style::new().bg(ctx.ui.stylesheet.accent_color_dark).bold());

    let [list_area, profile_area] = area.layout(
        &Layout::new(
            Direction::Horizontal,
            [Constraint::Percentage(40), Constraint::Fill(1)],
        )
        .spacing(Spacing::Overlap(1)),
    );
    let list_area = render_groupbox(ctx, Some("profiles"), list_area, false);

    if list.is_empty() {
        let paragraph = Paragraph::new(
            "Create a new profile by pressing 'n'.".fg(ctx.ui.stylesheet.accent_color),
        )
        .wrap(Wrap { trim: false });
        let area = list_area.centered(
            Constraint::Length(paragraph.line_width().min(list_area.width as usize) as u16),
            Constraint::Length(paragraph.line_count(list_area.width) as u16),
        );
        ctx.frame.render_widget(paragraph, area);
    } else {
        ctx.frame.render_stateful_widget(
            list,
            list_area,
            &mut ListState::default().with_selected(selected),
        );
    }

    render_profile_details_window(ctx, profile_area)
}

fn render_music_tab(ctx: &mut RenderContext<'_, '_>, area: Rect) {
    render_groupbox(ctx, None, area, false);

    if ctx.state.device_state.is_empty() {
        let paragraph =
            Paragraph::new("Detecting devices... ⏳".fg(ctx.ui.stylesheet.accent_color))
                .wrap(Wrap { trim: false });
        let area = area.centered(
            Constraint::Length(paragraph.line_width().min(area.width as usize) as u16),
            Constraint::Length(paragraph.line_count(area.width) as u16),
        );
        ctx.frame.render_widget(paragraph, area);
    } else {
        let [volume_area, music_area] = area.layout(
            &Layout::new(
                Direction::Horizontal,
                [Constraint::Percentage(40), Constraint::Fill(1)],
            )
            .spacing(Spacing::Overlap(1)),
        );

        let volume_area = render_groupbox(ctx, Some("volume"), volume_area, false);
        let volume_chart = BarChart::horizontal(
            ctx.state
                .sorted_device_state_iter()
                .map(|(device_id, device)| {
                    let selected = ctx.ui.selected_device.is_some_and(|id| *device_id == id);

                    match device.volume.as_ref() {
                        Some(volume) => Bar::with_label(
                            format!("{:>3} ({:>6.1} dB)", volume.volume, volume.db)
                                .set_style(if selected {
                                    Style::new().bg(ctx.ui.stylesheet.accent_color_dark).bold()
                                } else {
                                    Style::new().bg(ctx.ui.stylesheet.background_color)
                                })
                                .fg(ctx.ui.stylesheet.text_color),
                            volume.volume as u64,
                        )
                        .text_value("")
                        .fg(uuid_to_color(*device_id)),
                        None => Bar::new(0),
                    }
                })
                .collect::<Vec<_>>(),
        )
        .bar_width(1)
        .bar_gap(0)
        .max(MAX_VOLUME_LEVEL.into());
        ctx.frame.render_widget(volume_chart, volume_area);

        let music_area = render_groupbox(ctx, Some("music"), music_area, false);
        if let Some(device) = ctx
            .ui
            .selected_device
            .and_then(|device_id| ctx.state.find_device(&device_id))
        {
            let mut lines: Vec<Line> = Vec::new();

            if let Some(status) = device.status.as_ref() {
                lines.push(Line::from(vec![
                    format!("{}", status.state)
                        .fg(match status.state {
                            DevicePlaybackState::Play | DevicePlaybackState::Stream => {
                                ctx.ui.stylesheet.success_color
                            }
                            DevicePlaybackState::Pause | DevicePlaybackState::Stop => {
                                ctx.ui.stylesheet.highlight_color
                            }
                            _ => ctx.ui.stylesheet.text_color_sub,
                        })
                        .bold(),
                ]));

                if let Some(title) = status.title1.as_ref() {
                    lines.push(Line::from(""));
                    lines.push(Line::from("track".fg(ctx.ui.stylesheet.text_color_sub)));
                    lines.push(Line::from(
                        Span::raw(title).fg(ctx.ui.stylesheet.text_color),
                    ));
                }

                if let Some(artist) = status.artist.as_ref() {
                    lines.push(Line::from(""));
                    lines.push(Line::from("artist".fg(ctx.ui.stylesheet.text_color_sub)));
                    lines.push(Line::from(
                        Span::raw(artist).fg(ctx.ui.stylesheet.text_color),
                    ));
                }

                if let Some(album) = status.album.as_ref() {
                    lines.push(Line::from(""));
                    lines.push(Line::from("album".fg(ctx.ui.stylesheet.text_color_sub)));
                    lines.push(Line::from(
                        Span::raw(album).fg(ctx.ui.stylesheet.text_color),
                    ));
                }

                if let Some(service) = status.service.as_ref() {
                    lines.push(Line::from(""));
                    lines.push(Line::from(vec![
                        format!("via {service}").fg(ctx.ui.stylesheet.text_color_sub),
                    ]));
                }

                let progress_bar = if let Some(status_updated_at) = device.status_updated_at
                    && let Some(end) = status.totlen
                    && let Some(secs) = status.secs
                {
                    let offset = if ctx.state.is_device_playing(&device.device.id) {
                        status_updated_at.elapsed().as_secs()
                    } else {
                        0
                    };
                    let seek = (secs + offset).min(end);
                    let label = Span::styled(
                        format!(
                            "{} / {}",
                            format_minutes_seconds(seek),
                            format_minutes_seconds(end)
                        ),
                        Style::new()
                            .italic()
                            .bold()
                            .fg(ctx.ui.stylesheet.text_color),
                    );
                    Some(
                        Gauge::default()
                            .ratio(seek as f64 / end as f64)
                            .gauge_style(
                                Style::new()
                                    .fg(ctx.ui.stylesheet.accent_color_dark)
                                    .bg(ctx.ui.stylesheet.text_color_sub),
                            )
                            .label(label)
                            .use_unicode(true),
                    )
                } else {
                    None
                };

                #[cfg(feature = "ui-enable-image")]
                {
                    let [music_image_area, music_info_area] = music_area.layout(
                        &Layout::new(
                            Direction::Horizontal,
                            [Constraint::Percentage(40), Constraint::Fill(1)],
                        )
                        .spacing(2),
                    );

                    ctx.frame.render_stateful_widget(
                        widgets::Image::new(),
                        music_image_area,
                        &mut ctx.ui.music_image,
                    );
                    if let Some(progress_bar) = progress_bar {
                        let [top, bottom] = music_info_area.layout(
                            &Layout::new(
                                Direction::Vertical,
                                [Constraint::Length(1), Constraint::Fill(1)],
                            )
                            .spacing(1),
                        );
                        ctx.frame.render_widget(progress_bar, top);
                        ctx.frame.render_widget(
                            Paragraph::new(lines).wrap(Wrap { trim: false }),
                            bottom,
                        );
                    } else {
                        ctx.frame.render_widget(
                            Paragraph::new(lines).wrap(Wrap { trim: false }),
                            music_info_area,
                        );
                    }
                }
                #[cfg(not(feature = "ui-enable-image"))]
                {
                    if let Some(progress_bar) = progress_bar {
                        let [top, bottom] = music_area.layout(
                            &Layout::new(
                                Direction::Vertical,
                                [Constraint::Length(1), Constraint::Fill(1)],
                            )
                            .spacing(1),
                        );
                        ctx.frame.render_widget(progress_bar, top);
                        ctx.frame.render_widget(
                            Paragraph::new(lines).wrap(Wrap { trim: false }),
                            bottom,
                        );
                    } else {
                        ctx.frame.render_widget(
                            Paragraph::new(lines).wrap(Wrap { trim: false }),
                            music_area,
                        );
                    }
                }
            } else {
                let paragraph =
                    Paragraph::new("Loading... ⏳".fg(ctx.ui.stylesheet.text_color_sub))
                        .wrap(Wrap { trim: false });
                let area = area.centered(
                    Constraint::Length(paragraph.line_width().min(area.width as usize) as u16),
                    Constraint::Length(paragraph.line_count(area.width) as u16),
                );
                ctx.frame.render_widget(paragraph, area);
            }
        } else {
            let paragraph = Paragraph::new("Select a device.".fg(ctx.ui.stylesheet.text_color_sub))
                .wrap(Wrap { trim: false });
            let area = area.centered(
                Constraint::Length(paragraph.line_width().min(area.width as usize) as u16),
                Constraint::Length(paragraph.line_count(area.width) as u16),
            );
            ctx.frame.render_widget(paragraph, area);
        }
    }
}

#[cfg(feature = "ui-enable-logs")]
fn render_logs_tab(ctx: &mut RenderContext<'_, '_>, area: Rect) {
    use ansi_to_tui::IntoText;

    let area = render_groupbox(ctx, None, area, false);
    let lines: Vec<_> = ctx
        .state
        .logs
        .iter()
        .filter_map(|log| log.as_bytes().into_text().ok())
        .flat_map(|text| text.into_iter())
        .collect();

    ctx.frame
        .render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), area);
}

fn render_groupbox(
    ctx: &mut RenderContext<'_, '_>,
    title: Option<&str>,
    area: Rect,
    highlight: bool,
) -> Rect {
    let mut groupbox = Block::bordered()
        .border_type(BorderType::Rounded)
        .merge_borders(MergeStrategy::Fuzzy)
        .border_style(Style::default().fg(ctx.ui.stylesheet.border_color));

    if let Some(title) = title {
        groupbox = groupbox
            .title(Line::from(vec![
                "─ ".fg(ctx.ui.stylesheet.border_color),
                title.fg(ctx.ui.stylesheet.accent_color),
                if highlight {
                    format!(" {HIGHLIGHT_SYMBOL} ").fg(ctx.ui.stylesheet.highlight_color)
                } else {
                    " ".fg(ctx.ui.stylesheet.border_color)
                },
            ]))
            .title_alignment(Alignment::Left)
    } else if highlight {
        groupbox = groupbox.title(Line::from(vec![
            format!(" {HIGHLIGHT_SYMBOL} ").fg(ctx.ui.stylesheet.highlight_color),
        ]))
    }

    ctx.frame.render_widget(groupbox, area);

    area.inner(Margin {
        vertical: 2,
        horizontal: 2,
    })
}

fn _render_vertical_scrollbar(
    frame: &mut Frame,
    content_length: usize,
    position: usize,
    area: Rect,
) {
    let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight);
    let mut state = ScrollbarState::new(content_length).position(position);

    frame.render_stateful_widget(
        scrollbar,
        area.inner(Margin {
            vertical: 0,
            horizontal: 0,
        }),
        &mut state,
    );
}
