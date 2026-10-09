#![allow(unused)]
use ratatui::style::Color;
use uuid::Uuid;

pub fn first_after<T, I, F>(iter: I, mut pred: F) -> Option<T>
where
    I: IntoIterator<Item = T>,
    F: FnMut(&T) -> bool,
{
    let mut seen = false;

    iter.into_iter().find_map(|x| {
        if seen {
            Some(x)
        } else if pred(&x) {
            seen = true;
            None
        } else {
            None
        }
    })
}
/// Advances to the value after `current`, returning `None` after the last value. If `current`
/// is `None` the first value is selected.
pub fn select_next_or_none<T, I, F, R>(iter: I, map: F, current: Option<R>) -> Option<R>
where
    I: IntoIterator<Item = T> + Clone,
    F: FnMut(T) -> R + Clone,
    R: PartialEq + Eq,
{
    if current.is_none() {
        return select_first(iter, map);
    }

    first_after(
        iter.clone().into_iter().map(map.clone()),
        |p| matches!(&current, Some(x) if x == p),
    )
}

/// Advances to the value after `current`, wrapping to the first value after the last. If `current`
/// is `None` the first value is selected.
pub fn select_next<T, I, F, R>(iter: I, map: F, current: Option<R>) -> Option<R>
where
    I: IntoIterator<Item = T> + Clone,
    F: FnMut(T) -> R + Clone,
    R: PartialEq + Eq,
{
    first_after(
        iter.clone().into_iter().map(map.clone()),
        |p| matches!(&current, Some(x) if x == p),
    )
    .or_else(|| select_first(iter, map))
}

/// Moves back to the value before `current`, returning `None` after the first value. If `current`
/// is `None` the last value is selected.
pub fn select_previous_or_none<T, I, F, R>(iter: I, map: F, current: Option<R>) -> Option<R>
where
    I: IntoIterator<Item = T> + Clone,
    F: FnMut(T) -> R + Clone,
    R: PartialEq + Eq,
    <I as IntoIterator>::IntoIter: DoubleEndedIterator,
{
    if current.is_none() {
        return select_last(iter, map);
    }

    first_after(
        iter.clone().into_iter().rev().map(map.clone()),
        |p| matches!(&current, Some(x) if x == p),
    )
}

/// Moves back to the value before `current`, wrapping to the last value after the first. If `current`
/// is `None` the last value is selected.
pub fn select_previous<T, I, F, R>(iter: I, map: F, current: Option<R>) -> Option<R>
where
    I: IntoIterator<Item = T> + Clone,
    F: FnMut(T) -> R + Clone,
    R: PartialEq + Eq,
    <I as IntoIterator>::IntoIter: DoubleEndedIterator,
{
    first_after(
        iter.clone().into_iter().rev().map(map.clone()),
        |p| matches!(&current, Some(x) if x == p),
    )
    .or_else(|| select_last(iter, map))
}

pub fn select_first<T, I, F, R>(iter: I, map: F) -> Option<R>
where
    I: IntoIterator<Item = T>,
    F: FnMut(T) -> R,
{
    iter.into_iter().map(map).next()
}

pub fn select_last<T, I, F, R>(iter: I, map: F) -> Option<R>
where
    I: IntoIterator<Item = T>,
    F: FnMut(T) -> R,
    <I as IntoIterator>::IntoIter: DoubleEndedIterator,
{
    iter.into_iter().rev().map(map).next()
}

pub fn cycle_or_none<T, I>(current: &Option<T>, values: I) -> Option<T>
where
    T: PartialEq,
    I: IntoIterator<Item = T>,
{
    let mut iter = values.into_iter();
    match current {
        None => iter.next(),
        Some(current) => {
            iter.find(|value| value == current);
            iter.next()
        }
    }
}

pub fn uuid_to_color<UUID: Into<Uuid>>(uuid: UUID) -> Color {
    let uuid: Uuid = uuid.into();
    let bytes = uuid.as_bytes();

    let r = bytes[0] ^ bytes[8];
    let g = bytes[1] ^ bytes[9];
    let b = bytes[2] ^ bytes[10];

    Color::Rgb(r, g, b)
}

pub fn format_minutes_seconds(total_seconds: u64) -> String {
    let minutes = total_seconds / 60;
    let seconds = total_seconds % 60;

    format!("{}:{:02}", minutes, seconds)
}
