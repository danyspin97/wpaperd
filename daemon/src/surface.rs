use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use color_eyre::eyre::{bail, ensure, Context};
use color_eyre::Result;
use image::imageops::FilterType;
use image::{open, DynamicImage, ImageBuffer, Pixel, Rgba};
use log::{info, warn};
use smithay_client_toolkit::compositor::Region;
use smithay_client_toolkit::output::OutputInfo;
use smithay_client_toolkit::reexports::calloop::timer::{TimeoutAction, Timer};
use smithay_client_toolkit::reexports::calloop::LoopHandle;
use smithay_client_toolkit::reexports::client::protocol::wl_output::WlOutput;
use smithay_client_toolkit::reexports::client::protocol::{wl_shm, wl_surface};
use smithay_client_toolkit::reexports::client::QueueHandle;
use smithay_client_toolkit::shell::wlr_layer::{Anchor, Layer, LayerSurface};
use smithay_client_toolkit::shm::slot::SlotPool;
use walkdir::WalkDir;

use crate::wallpaper_info::WallpaperInfo;
use crate::wpaperd::Wpaperd;

pub struct Surface {
    pub surface: wl_surface::WlSurface,
    pub output: WlOutput,
    pub layer: LayerSurface,
    pub dimensions: (u32, u32),
    pub scale: i32,
    pub pool: SlotPool,
    pub wallpaper_info: Arc<WallpaperInfo>,
    pub need_redraw: bool,
    pub timer_expired: bool,
    pub time_changed: Instant,
    pub current_img: PathBuf,
    pub info: OutputInfo,
    pub configured: bool,
    pub idx: usize,
}

impl Surface {
    pub fn new(
        qh: &QueueHandle<Wpaperd>,
        wpaperd: &Wpaperd,
        output: WlOutput,
        surface: wl_surface::WlSurface,
        info: OutputInfo,
        wallpaper_info: Arc<WallpaperInfo>,
    ) -> Self {
        // TODO: error handling
        let layer = wpaperd.layer_state.create_layer_surface(
            qh,
            surface.clone(),
            Layer::Background,
            Some(format!("wpaperd-{}", info.name.as_ref().unwrap())),
            Some(&output),
        );
        layer.set_anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT | Anchor::BOTTOM);
        layer.set_exclusive_zone(-1);
        layer.set_size(0, 0);

        // Wayland clients are expected to render the cursor on their input region. By setting the
        // input region to an empty region, the compositor renders the default cursor. Without
        // this, and empty desktop won't render a cursor.
        let empty_region = Region::new(&wpaperd.compositor_state).unwrap();
        surface.set_input_region(Some(empty_region.wl_region()));

        // From `wl_surface::set_opaque_region`:
        // > Setting the pending opaque region has copy semantics, and the
        // > wl_region object can be destroyed immediately.
        empty_region.wl_region().destroy();

        // Commit the surface
        surface.commit();
        let pool = SlotPool::new(1200, &wpaperd.shm_state).unwrap();
        Self {
            output,
            layer,
            dimensions: (0, 0),
            scale: info.scale_factor,
            pool,
            surface,
            info,
            wallpaper_info,
            need_redraw: false,
            timer_expired: true,
            time_changed: Instant::now(),
            current_img: PathBuf::from("/"),
            configured: false,
            idx: 0,
        }
    }

    /// Returns true if something has been drawn to the surface
    pub fn draw(&mut self, now: &Instant) -> Result<()> {
        // No need to draw yet
        if (self.dimensions.0 == 0 || self.dimensions.1 == 0)
            || (!self.need_redraw && !self.timer_expired)
        {
            return Ok(());
        }

        let stride = 4 * self.dimensions.0 as i32 * self.scale;
        let width = self.dimensions.0 as i32 * self.scale;
        let height = self.dimensions.1 as i32 * self.scale;
        let size = (stride * height) as usize;

        // let egl_window = WlEglSurface::new(self.surface.id(), width, height);

        self.pool
            .resize(size)
            .context("resizing the wayland pool")?;
        let slot = self.pool.new_slot((stride * height) as usize)?;

        let buffer =
            self.pool
                .create_buffer_in(&slot, width, height, stride, wl_shm::Format::Abgr8888)?;
        if self.configured {
            let image = self.get_image(self.timer_expired, now)?;

            let mut image = image
                .resize_to_fill(width.try_into()?, height.try_into()?, FilterType::Lanczos3)
                .into_rgba8();

            self.apply_shadow(&mut image, width.try_into()?);

            let canvas = slot.canvas(&mut self.pool).unwrap();

            let mut writer = BufWriter::new(canvas);
            writer
                .write_all(image.as_raw())
                .context("writing the image to the surface")?;
            writer.flush().context("flushing the surface writer")?;
        }

        // Attach the buffer to the surface and mark the entire surface as damaged
        self.surface.attach(Some(buffer.wl_buffer()), 0, 0);
        self.surface.damage_buffer(0, 0, width, height);

        // Finally, commit the surface
        self.surface.commit();

        // Update status
        self.need_redraw = false;
        self.timer_expired = false;
        Ok(())
    }

    fn apply_shadow(&self, image: &mut ImageBuffer<Rgba<u8>, Vec<u8>>, width: u32) {
        if self.wallpaper_info.apply_shadow.unwrap_or(false) {
            const GRADIENT_HEIGHT: u32 = 11;
            type RgbaImage = image::ImageBuffer<image::Rgba<u8>, Vec<u8>>;
            let gradient = DynamicImage::ImageRgba8(
                RgbaImage::from_raw(
                    1,
                    GRADIENT_HEIGHT,
                    vec![
                        0, 0, 0, 225, 0, 0, 0, 202, 0, 0, 0, 178, 0, 0, 0, 154, 0, 0, 0, 130, 0, 0,
                        0, 107, 0, 0, 0, 83, 0, 0, 0, 59, 0, 0, 0, 36, 0, 0, 0, 12, 0, 0, 0, 0,
                    ],
                )
                .unwrap(),
            )
            .resize_exact(
                width,
                GRADIENT_HEIGHT * 4 * self.scale as u32,
                FilterType::Triangle,
            )
            .into_rgba8();

            image
                .pixels_mut()
                .zip(gradient.pixels())
                .for_each(|(p, g)| p.blend(g));
        }
    }

    fn get_image(
        &mut self,
        update: bool,
        now: &Instant,
    ) -> Result<DynamicImage, color_eyre::Report> {
        let path = self.wallpaper_info.path.as_ref().unwrap();
        let mut tries = 0;
        if path.is_dir() {
            if !update {
                if let Ok(image) = open(&self.current_img) {
                    return Ok(image);
                }
            }
            loop {
                let files: Vec<PathBuf> = WalkDir::new(path)
                    .into_iter()
                    .filter_map(|e| e.ok())
                    .filter(|e| {
                        if let Some(guess) = new_mime_guess::from_path(e.path()).first() {
                            guess.type_() == "image"
                        } else {
                            false
                        }
                    })
                    .map(|e| e.path().to_path_buf())
                    .collect();

                if files.is_empty() {
                    // There are no images, forcefully break out of the loop
                    // modulus operator panics when the right argument is 0
                    bail!("Directory {path:?} is empty");
                }

                // Reset our index if we'd be out of bounds
                if self.idx >= files.len() {
                    self.idx = 0
                };

                // This is a bit unweildy. We're sorting every time thru this loop. But since we're
                // walking the directory each time, I guess it's not that bad. Sorting can't be slower
                // than reading paths off the disk, right?
                let img_path = if self.wallpaper_info.sorting.eq(&Some(String::from("natural"))) {
                    // You can't sort an immutable Vec, so we have to clone it as mutable.
                    let mut sorted: Vec<PathBuf> = files.clone();
                    sorted.sort();
                    sorted[self.idx].clone()
                } else if self.wallpaper_info.sorting.eq(&Some(String::from("reverse"))) {
                    // You can't sort an immutable Vec, so we have to clone it as mutable.
                    let mut reversed: Vec<PathBuf> = files.clone();
                    reversed.sort_by(|a,b| b.cmp(a));
                    reversed[self.idx].clone()
                } else {
                    files[rand::random::<usize>() % files.len()].clone()
                };

                match open(&img_path).with_context(|| format!("opening the image {img_path:?}")) {
                    Ok(image) => {
                        info!("New image for monitor {:?}: {img_path:?}", self.name());
                        self.time_changed = *now;
                        self.current_img = img_path;
                        // Only ever up. We reverse the order of the files rather than count down
                        self.idx += 1;
                        break Ok(image);
                    }
                    Err(err) => {
                        warn!("{err:?}");
                        tries += 1;
                    }
                }

                ensure!(
                    tries < 5,
                    "tried reading an image from the directory {path:?} without success",
                );
            }
        } else {
            open(path).with_context(|| format!("opening the image {:?}", &path))
        }
    }

    /// Update the wallpaper_info of this Surface
    /// return true if the duration has changed
    pub fn update_wallpaper_info(&mut self, wallpaper_info: Arc<WallpaperInfo>) -> bool {
        let mut duration_changed = false;
        if self.wallpaper_info != wallpaper_info {
            if self.wallpaper_info.duration != wallpaper_info.duration {
                duration_changed = true;
            }
            self.wallpaper_info = wallpaper_info;
        }

        duration_changed
    }

    pub fn update_duration(&mut self, handle: LoopHandle<Wpaperd>, now: &Instant) {
        if self.check_duration(now) {
            self.set_next_duration(handle);
        }
    }

    /// Check if enough time has passed since we have drawn a wallpaper
    pub fn check_duration(&mut self, now: &Instant) -> bool {
        if let Some(duration) = self.wallpaper_info.duration {
            let time_passed = now.checked_duration_since(self.time_changed).unwrap();
            if duration.saturating_sub(time_passed) == std::time::Duration::ZERO {
                self.timer_expired = true;
                return true;
            }
        }

        false
    }

    /// Add the next timer in the event_loop for the current duration
    pub(crate) fn set_next_duration(&self, handle: LoopHandle<Wpaperd>) {
        if let Some(duration) = self.wallpaper_info.duration {
            let timer = Timer::from_duration(duration);
            handle
                .insert_source(timer, |_deadline, _: &mut (), _shared_data| {
                    TimeoutAction::Drop
                })
                .expect("Failed to insert event source!");
        }
    }

    pub fn name(&self) -> &str {
        self.info.name.as_ref().unwrap()
    }
}
