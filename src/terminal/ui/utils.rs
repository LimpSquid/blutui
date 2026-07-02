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

pub fn select_next<T, I, F, R>(iter: I, map: F, current: Option<R>) -> Option<R>
where
    I: IntoIterator<Item = T> + Clone,
    F: FnMut(T) -> R + Clone,
    R: PartialEq + Eq + Clone,
{
    first_after(
        iter.clone().into_iter().map(map.clone()),
        |p| matches!(&current, Some(x) if x == p),
    )
    .or_else(|| select_first(iter, map))
}

pub fn select_previous<T, I, F, R>(iter: I, map: F, current: Option<R>) -> Option<R>
where
    I: IntoIterator<Item = T> + Clone,
    F: FnMut(T) -> R + Clone,
    R: PartialEq + Eq + Clone,
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

pub fn uuid_to_color<UUID: Into<Uuid>>(uuid: UUID) -> Color {
    let uuid: Uuid = uuid.into();
    let bytes = uuid.as_bytes();

    let r = bytes[0] ^ bytes[8];
    let g = bytes[1] ^ bytes[9];
    let b = bytes[2] ^ bytes[10];

    Color::Rgb(r, g, b)
}
