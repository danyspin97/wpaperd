use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use color_eyre::eyre::{bail, ensure, Context};
use color_eyre::Result;
use image::imageops::FilterType;
use image::{open, DynamicImage, ImageBuffer, Pixel, Rgba};
use log::{info, warn};
use smithay_client_toolkit::reexports::calloop::timer::{TimeoutAction, Timer};
use smithay_client_toolkit::reexports::calloop::LoopHandle;
use smithay_client_toolkit::reexports::client::protocol::wl_output::WlOutput;
use smithay_client_toolkit::reexports::client::protocol::wl_surface;
use smithay_client_toolkit::reexports::client::Proxy;
use smithay_client_toolkit::shell::wlr_layer::{LayerSurface, LayerSurfaceConfigure};
use walkdir::WalkDir;
use wayland_egl::WlEglSurface;

use crate::render::{EglContext, Renderer};
use crate::wallpaper_info::Sorting;
use crate::wallpaper_info::WallpaperInfo;
use crate::wpaperd::Wpaperd;
use khronos_egl::API as egl;

pub struct Surface {
    pub name: String,
    pub surface: wl_surface::WlSurface,
    pub output: WlOutput,
    pub layer: LayerSurface,
    pub dimensions: (u32, u32),
    pub scale: i32,
    pub wallpaper_info: Arc<WallpaperInfo>,
    pub need_redraw: bool,
    pub timer_expired: bool,
    pub time_changed: Instant,
    pub current_img: PathBuf,
    pub configured: bool,
    pub image_paths: Vec<PathBuf>,
    pub current_index: usize,
    egl_context: EglContext,
}

impl Surface {
    pub fn new(
        name: String,
        layer: LayerSurface,
        output: WlOutput,
        surface: wl_surface::WlSurface,
        scale_factor: i32,
        wallpaper_info: Arc<WallpaperInfo>,
        egl_display: egl::Display,
    ) -> Self {
        // Commit the surface
        surface.commit();

        Self {
            name,
            output,
            layer,
            dimensions: (0, 0),
            scale: scale_factor,
            surface,
            wallpaper_info,
            need_redraw: false,
            timer_expired: true,
            time_changed: Instant::now(),
            current_img: PathBuf::from("/"),
            configured: false,
            image_paths: Vec::new(),
            current_index: 0,
            egl_context: EglContext::new(egl_display),
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

        let width = self.dimensions.0 as i32 * self.scale;
        let height = self.dimensions.1 as i32 * self.scale;
        if self.configured {
            let image = self.get_image(self.timer_expired, now)?;

            let mut image = image
                .resize_to_fill(width.try_into()?, height.try_into()?, FilterType::Lanczos3)
                .into_rgba8();

            self.apply_shadow(&mut image, width.try_into()?);
            let wl_egl_surface = WlEglSurface::new(self.surface.id(), width, height).unwrap();
            let egl_surface = unsafe {
                egl.create_window_surface(
                    self.egl_context.display,
                    self.egl_context.config,
                    wl_egl_surface.ptr() as egl::NativeWindowType,
                    None,
                )
                .expect("unable to create an EGL surface")
            };

            egl.make_current(
                self.egl_context.display,
                Some(egl_surface),
                Some(egl_surface),
                Some(self.egl_context.context),
            )
            .expect("unable to bind the context");

            let renderer = unsafe { Renderer::new() }.expect("unable to create a renderer");
            renderer.resize(width, height);
            unsafe { renderer.draw(image.into())? };

            egl.swap_buffers(self.egl_context.display, egl_surface)
                .expect("unable to post the surface content");

            renderer.clear_after_draw()?;
        }

        // Mark the entire surface as damaged
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

    /// Get index for the next image based on the sorting method
    fn get_new_image_index(&self, files: &Vec<PathBuf>) -> usize {
        match self.wallpaper_info.sorting {
            Sorting::Random => rand::random::<usize>() % files.len(),
            Sorting::Ascending => {
                let idx = match files.binary_search(&self.current_img) {
                    // Perform increment here, do validation/bounds checking below
                    Ok(n) => n + 1,
                    Err(err) => {
                        info!(
                            "Current image not found, defaulting to first image ({:?})",
                            err
                        );
                        // set idx to > slice length so the guard sets it correctly for us
                        files.len()
                    }
                };

                if idx >= files.len() {
                    0
                } else {
                    idx
                }
            }
            Sorting::Descending => {
                let idx = match files.binary_search(&self.current_img) {
                    Ok(n) => n,
                    Err(err) => {
                        info!(
                            "Current image not found, defaulting to last image ({:?})",
                            err
                        );
                        files.len()
                    }
                };

                // Here, bounds checking is strictly ==, as we cannot go lower than 0 for usize
                if idx == 0 {
                    files.len() - 1
                } else {
                    idx - 1
                }
            }
        }
    }

    fn get_image_files_from_dir(&self, dir_path: &PathBuf) -> Vec<PathBuf> {
        WalkDir::new(dir_path)
            .sort_by_file_name()
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
            .collect()
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
                let files = self.get_image_files_from_dir(path);

                // There are no images, forcefully break out of the loop
                if files.is_empty() {
                    bail!("Directory {path:?} does not contain any valid image files.");
                }

                let is_below_len =
                    !self.image_paths.is_empty() && self.current_index < self.image_paths.len();

                let img_path = if is_below_len && self.image_paths[self.current_index].is_file() {
                    self.image_paths[self.current_index].clone()
                } else {
                    // Select new image based on sorting method
                    let index = self.get_new_image_index(&files);
                    files[index].clone()
                };

                match open(&img_path).with_context(|| format!("opening the image {img_path:?}")) {
                    Ok(image) => {
                        info!("New image for monitor {:?}: {img_path:?}", self.name());

                        if !self.image_paths.contains(&img_path) {
                            self.image_paths.push(img_path.clone());
                            self.current_index = self.image_paths.len() - 1;
                        };

                        self.current_img = img_path;
                        self.time_changed = *now;

                        break Ok(image);
                    }
                    Err(err) => {
                        warn!("{err:?}");
                        tries += 1;
                    }
                };

                ensure!(
                    tries < 5,
                    "tried reading an image from the directory {path:?} without success",
                );
            }
        } else {
            open(path).with_context(|| format!("opening the image {:?}", &path))
        }
    }

    /// Update wallpaper by going down 1 index through the cached image paths
    /// Expiry timer reset even if already at the first cached image
    pub fn previous_image(&mut self) {
        self.timer_expired = true;

        if self.current_index == 0 {
            return;
        };

        self.current_index -= 1;
    }

    /// Update wallpaper by going up 1 index through the cached image paths
    pub fn next_image(&mut self) {
        self.timer_expired = true;

        if self.current_index > self.image_paths.len() {
            return;
        };

        self.current_index += 1;
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
                self.next_image();
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
        &self.name
    }

    pub fn resize(&mut self, configure: LayerSurfaceConfigure) {
        self.dimensions = configure.new_size;
        self.need_redraw = true;
        // self.renderer.resize(
        //     self.dimensions.0.try_into().unwrap(),
        //     self.dimensions.1.try_into().unwrap(),
        // );
    }
}
