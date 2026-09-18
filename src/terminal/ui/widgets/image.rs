use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::StatefulWidget;
use ratatui_image::picker::Picker;
use ratatui_image::protocol::StatefulProtocol;
use ratatui_image::{Resize, ResizeEncodeRender};
use tokio::sync::oneshot;

use super::super::Redraw;
use crate::image_cache;

struct ResizeJob {
    generation: u64,
    receiver: oneshot::Receiver<StatefulProtocol>,
}

pub struct ImageState {
    image_id: Option<image_cache::ImageId>,
    protocol: Option<StatefulProtocol>,
    picker: Picker,
    resize: Resize,
    resize_job: Option<ResizeJob>,
    generation: u64,
    redraw: Redraw,
}

impl ImageState {
    pub fn new(redraw: Redraw) -> Self {
        Self {
            image_id: None,
            protocol: None,
            picker: Picker::halfblocks(),
            resize: Resize::Scale(None),
            resize_job: None,
            generation: 0,
            redraw,
        }
    }

    pub fn set_picker(&mut self, picker: Picker) {
        self.picker = picker;
    }

    pub fn set_image(&mut self, image: image_cache::Image) {
        if self.image_id == Some(image.id) {
            return;
        }

        self.invalidate();
        self.image_id = Some(image.id);
        self.protocol = Some(
            self.picker
                .new_resize_protocol(image.image.as_ref().to_owned()),
        );
    }

    pub fn clear_image(&mut self) {
        self.invalidate();
        self.image_id = None;
        self.protocol = None;
    }

    fn invalidate(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.resize_job = None;
    }

    fn poll_resize_job(&mut self) {
        let Some(mut job) = self.resize_job.take() else {
            return;
        };

        match job.receiver.try_recv() {
            Ok(protocol) => {
                if job.generation == self.generation {
                    self.protocol = Some(protocol);
                }
            }
            Err(oneshot::error::TryRecvError::Empty) => {
                self.resize_job = Some(job);
            }
            // Channel closed
            Err(_) => {}
        }
    }

    fn resize_if_needed(&mut self, area: Rect) -> Option<&mut StatefulProtocol> {
        let mut protocol = self.protocol.take()?;
        let Some(size) = protocol.needs_resize(&self.resize, area.into()) else {
            self.protocol = Some(protocol);
            return self.protocol.as_mut();
        };

        let resize = self.resize.clone();
        let redraw = self.redraw.clone();
        let generation = self.generation;
        let (sender, receiver) = oneshot::channel();

        tokio::task::spawn_blocking(move || {
            protocol.resize_encode(&resize, size);

            if sender.send(protocol).is_ok() {
                redraw.redraw_ui();
            }
        });

        self.resize_job = Some(ResizeJob {
            generation,
            receiver,
        });

        None
    }
}

#[derive(Debug, Default)]
pub struct Image;

impl Image {
    pub const fn new() -> Self {
        Self
    }
}

impl StatefulWidget for Image {
    type State = ImageState;

    fn render(self, area: Rect, buffer: &mut Buffer, state: &mut Self::State) {
        state.poll_resize_job();

        if let Some(protocol) = state.resize_if_needed(area).as_mut() {
            protocol.render(area, buffer);
        }
    }
}
