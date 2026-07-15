use std::time::Instant;

use chrono::{Duration, Utc};
use itertools::Itertools;
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Margin, Rect, Spacing};
use ratatui::style::{Style, Stylize};
use ratatui::symbols::merge::MergeStrategy;
use ratatui::text::{Line, Span};
use ratatui::widgets::canvas::Canvas;
use ratatui::widgets::{
    Bar, BarChart, Block, BorderType, List, ListState, Paragraph, Scrollbar, ScrollbarOrientation,
    ScrollbarState, Tabs, Wrap,
};
use strum::IntoEnumIterator;

use super::{Ui, components::*, stylesheet::*, utils::*};
use crate::bluos::MAX_VOLUME_LEVEL;
use crate::terminal::app::{AppState, BusyFlags, DeviceState};

struct RenderContext<'a, 'b> {
    frame: &'a mut Frame<'b>,
    state: &'a AppState,
    ui: &'a mut Ui,
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
    Audio,
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

    ui.render_start = Instant::now();
}

pub fn after_render(_: &AppState, ui: &mut Ui) {
    let render_time = ui.render_start.elapsed();

    tracing::trace!(?render_time, "render end");

    if render_time.as_millis() >= 10 {
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
            Span::from("●".fg(ctx.ui.stylesheet.highlight_color)),
            Rect::new(area.width.max(1) - 1, area.height.max(1) - 1, 1, 1),
        );
    }
}

#[tracing::instrument(skip_all)]
fn render_dialog(ctx: &mut RenderContext<'_, '_>, area: Rect) {
    if let Some(dialog) = ctx.ui.active_dialog.as_ref() {
        dialog.render_ref(area, ctx.frame.buffer_mut());
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

    ctx.frame
        .render_widget(Keybindings::new(&keybindings, ctx.ui.stylesheet), area);
}

fn render_discovered_devices_window(ctx: &mut RenderContext<'_, '_>, area: Rect) {
    let groupbox_area = render_groupbox(
        ctx,
        Some("devices"),
        area,
        ctx.ui.window_focus == WindowFocus::DiscoveredDevices,
    );

    if ctx.state.device_state.is_empty() {
        let text = Line::from("Detecting devices... ⏳".fg(ctx.ui.stylesheet.accent_color));
        let area = area.centered(
            Constraint::Length(text.width() as u16),
            Constraint::Length(1),
        );
        ctx.frame
            .render_widget(Paragraph::new(text).wrap(Wrap { trim: false }), area);
    } else {
        let mut selected = None;
        let list = ctx
            .state
            .sorted_device_state_iter()
            .map(|(_, state)| (&state.device, &state.group_status))
            .enumerate()
            .map(|(index, (device, group_status))| {
                let device_last_update = device.last_update;
                let device_name = device
                    .attributes
                    .first()
                    .and_then(|a| a.fields.get("name").cloned())
                    .unwrap_or_else(|| device.id.to_string());
                let device_model = device
                    .attributes
                    .iter()
                    .flat_map(|a| a.fields.iter())
                    .find(|(k, _)| *k == "model")
                    .map(|(_, v)| v.to_owned())
                    .unwrap_or("N/A".to_string());
                if ctx
                    .ui
                    .selected_device
                    .is_some_and(|device_id| device_id == device.id)
                {
                    selected = Some(index);
                }

                vec![Line::from(vec![
                    format!("{device_name}").fg(ctx.ui.stylesheet.text_color),
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
        status,
        volume,
        group_status,
        diagnostics,
        input_selection,
        audio_settings,
        player_settings,
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
                        (Some(ctn), None) => format!("{ctn}"),
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
        data.push((
            "service".to_string(),
            status.as_ref().map(|s| match s.service.as_ref() {
                Some(service) => format!("{} ({})", service, s.state,),
                None => "N/A".to_string(),
            }),
        ));
        data.push((
            "now playing".to_string(),
            status
                .as_ref()
                .map(|s| match (s.title1.as_ref(), s.title2.as_ref()) {
                    (Some(t1), Some(t2)) => format!("{t1} • {t2}"),
                    (Some(t1), None) => format!("{t1}"),
                    (None, Some(a)) => format!("{a}"),
                    (None, None) => "N/A".to_string(),
                }),
        ));
        data.push((
            "album".to_string(),
            status.as_ref().map(|s| match s.album.as_ref() {
                Some(a) => a.to_owned(),
                None => "N/A".to_string(),
            }),
        ));
        if let Some(input_selection) = input_selection
            && !input_selection.item.is_empty()
        {
            data.push((
                "input source".to_string(),
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

        let data_key_max_len = data
            .iter()
            .map(|(k, _)| k.chars().count())
            .max()
            .unwrap_or_default() as u16;
        let spacing = 1u16;
        let separator = ":";
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
                Paragraph::new(Line::from(key.as_str().fg(ctx.ui.stylesheet.text_color)))
                    .wrap(Wrap { trim: false }),
                horizontal_layout[0],
            );
            ctx.frame.render_widget(
                Line::from(separator.fg(ctx.ui.stylesheet.text_color_sub)),
                horizontal_layout[1],
            );
            ctx.frame.render_widget(
                Paragraph::new(Line::from(match value {
                    Some(value) => value.as_str().fg(ctx.ui.stylesheet.text_color_sub),
                    None => loading.fg(ctx.ui.stylesheet.text_color_sub),
                }))
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
        let area = area.centered(
            Constraint::Length(text.width() as u16),
            Constraint::Length(1),
        );
        ctx.frame
            .render_widget(Paragraph::new(text).wrap(Wrap { trim: false }), area);
    }
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
        Tab::Audio => render_audio_tab(ctx, layout[2]),
        #[cfg(feature = "ui-enable-logs")]
        Tab::Logs => render_logs_tab(ctx, layout[2]),
    }
}

fn render_profile_tab(ctx: &mut RenderContext<'_, '_>, area: Rect) {
    let layout = Layout::new(
        Direction::Horizontal,
        [Constraint::Percentage(35), Constraint::Fill(1)],
    )
    .spacing(Spacing::Overlap(1))
    .split(area);

    let list_area = render_groupbox(ctx, Some("profiles"), layout[0], false);
    let profile_area = render_groupbox(ctx, Some("profile details"), layout[1], false);

    let mut selected = None;
    let mut profile_in_yaml_or_err = None;
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
                profile_in_yaml_or_err = match &p.profile {
                    Ok(profile) => yaml_serde::to_string(profile).ok(),
                    Err(error) => Some(error.to_owned()),
                }
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

    if list.is_empty() {
        let text = Line::from("No profiles.".fg(ctx.ui.stylesheet.accent_color));
        let area = list_area.centered(
            Constraint::Length(text.width() as u16),
            Constraint::Length(1),
        );
        ctx.frame
            .render_widget(Paragraph::new(text).wrap(Wrap { trim: false }), area);
    } else {
        ctx.frame.render_stateful_widget(
            list,
            list_area,
            &mut ListState::default().with_selected(selected),
        );
    }

    if ctx
        .state
        .busy_flags
        .contains(BusyFlags::PROFILE_TRANSITIONING)
    {
        let text = "Applying profile... ⏳".fg(ctx.ui.stylesheet.accent_color);
        let area = profile_area.centered(
            Constraint::Length(text.width() as u16),
            Constraint::Length(1),
        );
        ctx.frame
            .render_widget(Paragraph::new(text).wrap(Wrap { trim: false }), area);
    } else if let Some(yaml) = profile_in_yaml_or_err {
        ctx.frame.render_widget(
            Paragraph::new(yaml).wrap(Wrap { trim: false }),
            profile_area,
        );
    } else {
        let text = "Select a profile.".fg(ctx.ui.stylesheet.accent_color);
        let area = profile_area.centered(
            Constraint::Length(text.width() as u16),
            Constraint::Length(1),
        );
        ctx.frame
            .render_widget(Paragraph::new(text).wrap(Wrap { trim: false }), area);
    }
}

fn render_audio_tab(ctx: &mut RenderContext<'_, '_>, area: Rect) {
    render_groupbox(ctx, None, area, false);

    if ctx.state.device_state.is_empty() {
        let text = Line::from("Detecting devices... ⏳".fg(ctx.ui.stylesheet.accent_color));
        let area = area.centered(
            Constraint::Length(text.width() as u16),
            Constraint::Length(1),
        );
        ctx.frame
            .render_widget(Paragraph::new(text).wrap(Wrap { trim: false }), area);
    } else {
        let [volume_area, other_area] = area.layout(
            &Layout::new(
                Direction::Horizontal,
                [Constraint::Percentage(50), Constraint::Fill(1)],
            )
            .spacing(Spacing::Overlap(1)),
        );
        let volume_area = render_groupbox(ctx, Some("volume"), volume_area, false);
        let _other_area = render_groupbox(ctx, Some("TODO"), other_area, false);

        let volume_chart = BarChart::horizontal(
            ctx.state
                .sorted_device_state_iter()
                .map(|(device_id, device)| match device.volume.as_ref() {
                    Some(volume) => {
                        Bar::with_label(format!("{:.1} db", volume.db), volume.volume as u64)
                            .fg(uuid_to_color(*device_id))
                    }
                    None => Bar::new(0),
                })
                .collect::<Vec<_>>(),
        )
        .bar_width(3)
        .bar_gap(0)
        .max(MAX_VOLUME_LEVEL.into());

        ctx.frame.render_widget(volume_chart, volume_area);
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
