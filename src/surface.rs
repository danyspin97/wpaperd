use std::cell::Cell;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use color_eyre::eyre::{ensure, Context};
use color_eyre::Result;
use dowser::Dowser;
use image::imageops::FilterType;
use image::open;
use log::warn;
use smithay_client_toolkit::{
    output::OutputInfo,
    reexports::{
        client::protocol::{wl_output, wl_shm, wl_surface},
        client::{Attached, Main},
        protocols::wlr::unstable::layer_shell::v1::client::{
            zwlr_layer_shell_v1, zwlr_layer_surface_v1,
        },
    },
    shm::AutoMemPool,
};
use wayland_client::protocol::wl_buffer::WlBuffer;

use crate::output::Output;

#[derive(PartialEq, Copy, Clone)]
enum RenderEvent {
    Configure { width: u32, height: u32 },
    Closed,
}

pub struct Surface {
    surface: wl_surface::WlSurface,
    layer_surface: Main<zwlr_layer_surface_v1::ZwlrLayerSurfaceV1>,
    next_render_event: Rc<Cell<Option<RenderEvent>>>,
    pub info: OutputInfo,
    pool: AutoMemPool,
    dimensions: (u32, u32),
    pub output: Arc<Output>,
    need_redraw: bool,
    buffer: Option<WlBuffer>,
    time_changed: Instant,
}

impl Surface {
    pub fn new(
        wl_output: &wl_output::WlOutput,
        surface: wl_surface::WlSurface,
        layer_shell: &Attached<zwlr_layer_shell_v1::ZwlrLayerShellV1>,
        info: OutputInfo,
        pool: AutoMemPool,
        output: Arc<Output>,
    ) -> Self {
        let layer_surface = layer_shell.get_layer_surface(
            &surface,
            Some(wl_output),
            zwlr_layer_shell_v1::Layer::Background,
            "example".to_owned(),
        );

        layer_surface.set_size(0, 0);
        layer_surface.set_anchor(
            zwlr_layer_surface_v1::Anchor::Top
                | zwlr_layer_surface_v1::Anchor::Left
                | zwlr_layer_surface_v1::Anchor::Right
                | zwlr_layer_surface_v1::Anchor::Bottom,
        );
        layer_surface.set_exclusive_zone(-1);

        let next_render_event = Rc::new(Cell::new(None::<RenderEvent>));
        let next_render_event_handle = Rc::clone(&next_render_event);
        layer_surface.quick_assign(move |layer_surface, event, _| {
            match (event, next_render_event_handle.get()) {
                (zwlr_layer_surface_v1::Event::Closed, _) => {
                    next_render_event_handle.set(Some(RenderEvent::Closed));
                }
                (
                    zwlr_layer_surface_v1::Event::Configure {
                        serial,
                        width,
                        height,
                    },
                    next,
                ) if next != Some(RenderEvent::Closed) => {
                    layer_surface.ack_configure(serial);
                    next_render_event_handle.set(Some(RenderEvent::Configure { width, height }));
                }
                (_, _) => {}
            }
        });

        // Commit so that the server will send a configure event
        surface.commit();

        Self {
            surface,
            layer_surface,
            next_render_event,
            info,
            pool,
            dimensions: (0, 0),
            need_redraw: false,
            output,
            buffer: None,
            time_changed: Instant::now(),
        }
    }

    /// Handles any events that have occurred since the last call, redrawing if needed.
    /// Returns true if the surface should be dropped.
    pub fn handle_events(&mut self) -> bool {
        match self.next_render_event.take() {
            Some(RenderEvent::Closed) => true,
            Some(RenderEvent::Configure { width, height }) => {
                if self.dimensions != (width, height) {
                    self.dimensions = (width, height);
                    self.need_redraw = true;
                }
                false
            }
            None => false,
        }
    }

    pub fn should_draw(&self, now: &Instant) -> bool {
        let timer_expired = if let Some(duration) = self.output.duration {
            now.checked_duration_since(self.time_changed).unwrap() > duration
        } else {
            false
        };
        (self.need_redraw || timer_expired) && self.dimensions.0 != 0
    }

    /// Returns true if something has been drawn to the surface
    pub fn draw(&mut self, now: Instant) -> Result<()> {
        let path = self.output.path.as_ref().unwrap();

        let stride = 4 * self.dimensions.0 as i32;
        let width = self.dimensions.0 as i32;
        let height = self.dimensions.1 as i32;

        self.pool
            .resize((stride * height) as usize)
            .context("resizing the wayland pool")?;

        let mut tries = 0;
        let image = if path.is_dir() {
            loop {
                let files = Vec::<PathBuf>::try_from(
                    Dowser::filtered(|p: &Path| {
                        if let Some(guess) = new_mime_guess::from_path(&p).first() {
                            guess.type_() == "image"
                        } else {
                            false
                        }
                    })
                    .with_path(path),
                )
                .with_context(|| format!("iterating files in directory {path:?}"))?;
                let img_path = files[rand::random::<usize>() % files.len()].clone();
                match open(&img_path).with_context(|| format!("opening the image {img_path:?}")) {
                    Ok(image) => {
                        break image;
                    }
                    Err(err) => {
                        warn!("{:?}", err);
                        tries += 1;
                    }
                }

                ensure!(
                    tries < 5,
                    "tried reading an image from the directory {path:?} without success",
                );
            }
        } else {
            let img_path = path.to_path_buf();
            open(&img_path).with_context(|| format!("opening the image {img_path:?}"))?
        };

        let image = image
            .resize_to_fill(width.try_into()?, height.try_into()?, FilterType::Lanczos3)
            .into_rgba8();

        if let Some(buffer) = &self.buffer {
            buffer.destroy();
        }

        self.buffer = Some(self.pool.try_draw::<_, color_eyre::eyre::Error>(
            width,
            height,
            stride,
            wl_shm::Format::Abgr8888,
            |canvas: &mut [u8]| {
                let mut writer = BufWriter::new(canvas);
                writer
                    .write_all(image.as_raw())
                    .context("writing the image to the surface")?;
                writer.flush().context("flushing the surface writer")?;

                Ok(())
            },
        )?);

        // Attach the buffer to the surface and mark the entire surface as damaged
        self.surface
            .attach(Some(self.buffer.as_ref().unwrap()), 0, 0);
        self.surface
            .damage_buffer(0, 0, width as i32, height as i32);

        // Finally, commit the surface
        self.surface.commit();

        // Update status
        self.need_redraw = false;
        self.time_changed = now;
        Ok(())
    }

    pub fn update_output(&mut self, output: Arc<Output>) {
        if output.path != self.output.path {
            self.need_redraw = true;
        }

        self.output = output;
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        self.layer_surface.destroy();
        self.surface.destroy();
    }
}
