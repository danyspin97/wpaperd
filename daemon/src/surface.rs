use std::{
    cell::RefCell,
    rc::Rc,
    time::{Duration, Instant},
};

use color_eyre::eyre::{Context, ContextCompat};
use color_eyre::Result;
use image::imageops::FilterType;
use image::{DynamicImage, ImageBuffer, Pixel, Rgba, RgbaImage};
use log::error;
use smithay_client_toolkit::output::OutputInfo;
use smithay_client_toolkit::reexports::calloop::timer::{TimeoutAction, Timer};
use smithay_client_toolkit::reexports::calloop::{LoopHandle, RegistrationToken};
use smithay_client_toolkit::reexports::client::protocol::wl_output::{Transform, WlOutput};
use smithay_client_toolkit::reexports::client::protocol::wl_surface;
use smithay_client_toolkit::reexports::client::QueueHandle;
use smithay_client_toolkit::shell::wlr_layer::{LayerSurface, LayerSurfaceConfigure};

use crate::image_picker::ImagePicker;
use crate::wallpaper_info::WallpaperInfo;
use crate::wpaperd::Wpaperd;
use crate::{
    filelist_cache::FilelistCache,
    render::{EglContext, Renderer},
};

#[derive(Debug)]
pub struct DisplayInfo {
    name: String,
    width: i32,
    height: i32,
    scale: i32,
    transform: Transform,
}

pub struct Surface {
    pub surface: wl_surface::WlSurface,
    pub output: WlOutput,
    pub layer: LayerSurface,
    egl_context: EglContext,
    renderer: Renderer,
    pub image_picker: ImagePicker,
    pub event_source: Option<RegistrationToken>,
    wallpaper_info: WallpaperInfo,
    info: Rc<RefCell<DisplayInfo>>,
    drawn: bool,
}

impl DisplayInfo {
    pub fn new(info: OutputInfo) -> Self {
        // let width = info.logical_size.unwrap().0;
        // let height = info.logical_size.unwrap().1;
        Self {
            name: info.name.unwrap(),
            width: 0,
            height: 0,
            scale: info.scale_factor,
            transform: info.transform,
        }
    }

    pub fn scaled_width(&self) -> i32 {
        self.width * self.scale
    }

    pub fn scaled_height(&self) -> i32 {
        self.height * self.scale
    }

    pub fn adjusted_width(&self) -> i32 {
        match self.transform {
            Transform::Normal | Transform::_180 | Transform::Flipped | Transform::Flipped180 => {
                self.width * self.scale
            }
            Transform::_90 | Transform::_270 | Transform::Flipped90 | Transform::Flipped270 => {
                self.height * self.scale
            }
            _ => unreachable!(),
        }
    }

    pub fn adjusted_height(&self) -> i32 {
        match self.transform {
            Transform::Normal | Transform::_180 | Transform::Flipped | Transform::Flipped180 => {
                self.height * self.scale
            }
            Transform::_90 | Transform::_270 | Transform::Flipped90 | Transform::Flipped270 => {
                self.width * self.scale
            }
            _ => unreachable!(),
        }
    }

    pub fn change_size(&mut self, configure: LayerSurfaceConfigure) -> bool {
        let new_width = configure.new_size.0 as i32;
        let new_height = configure.new_size.1 as i32;
        if (self.width, self.height) != (new_width, new_height) {
            self.width = new_width;
            self.height = new_height;
            true
        } else {
            false
        }
    }

    pub fn change_transform(&mut self, transform: Transform) -> bool {
        if self.transform != transform {
            self.transform = transform;
            true
        } else {
            false
        }
    }

    pub fn change_scale_factor(&mut self, scale_factor: i32) -> bool {
        if self.scale != scale_factor {
            self.scale = scale_factor;
            true
        } else {
            false
        }
    }
}

impl Surface {
    pub fn new(
        layer: LayerSurface,
        output: WlOutput,
        surface: wl_surface::WlSurface,
        info: DisplayInfo,
        wallpaper_info: WallpaperInfo,
        egl_display: egl::Display,
        filelist_cache: Rc<RefCell<FilelistCache>>,
    ) -> Self {
        let egl_context = EglContext::new(egl_display, &surface);
        // Make the egl context as current to make the renderer creation work
        egl_context.make_current().unwrap();

        // Commit the surface
        surface.commit();

        let image_picker = ImagePicker::new(&wallpaper_info, filelist_cache);

        let image = black_image();
        let info = Rc::new(RefCell::new(info));
        let renderer = unsafe { Renderer::new(image.into(), info.clone()).unwrap() };

        Self {
            output,
            layer,
            info,
            surface,
            egl_context,
            renderer,
            image_picker,
            event_source: None,
            wallpaper_info,
            drawn: false,
        }
    }

    /// Returns true if something has been drawn to the surface
    pub fn draw(&mut self, qh: &QueueHandle<Wpaperd>, time: u32) -> Result<()> {
        self.drawn = true;

        let info = self.info.borrow();
        let width = info.adjusted_width();
        let height = info.adjusted_height();

        // Use the correct context before loading the texture and drawing
        self.egl_context.make_current()?;

        if let Some(image) = self
            .image_picker
            .get_image_from_path(&self.wallpaper_info.path)?
        {
            let image = image.into_rgba8();
            self.renderer
                .load_wallpaper(image.into(), self.wallpaper_info.mode)?;
            self.renderer.start_animation(time);

            // self.apply_shadow(&mut image, width.try_into()?);
        }
        if self.renderer.time_started == 0 {
            self.renderer.start_animation(time);
        }

        unsafe { self.renderer.draw(time, self.wallpaper_info.mode)? };

        if self.is_drawing_animation(time) {
            self.queue_draw(qh);
        }

        self.renderer.clear_after_draw()?;
        self.egl_context.swap_buffers()?;

        // Reset the context
        egl::API
            .make_current(self.egl_context.display, None, None, None)
            .context("Resetting the GL context")?;

        // Mark the entire surface as damaged
        self.surface.damage_buffer(0, 0, width, height);

        // Finally, commit the surface
        self.surface.commit();

        Ok(())
    }

    fn _apply_shadow(&self, image: &mut ImageBuffer<Rgba<u8>, Vec<u8>>, width: u32) {
        if self.wallpaper_info.apply_shadow {
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
                GRADIENT_HEIGHT * 4 * self.info.borrow().scale as u32,
                FilterType::Triangle,
            )
            .into_rgba8();

            image
                .pixels_mut()
                .zip(gradient.pixels())
                .for_each(|(p, g)| p.blend(g));
        }
    }

    pub fn name(&self) -> String {
        self.info.borrow().name.to_string()
    }

    /// Resize the surface
    pub fn resize(&mut self, qh: &QueueHandle<Wpaperd>) {
        let info = self.info.borrow();
        let width = info.adjusted_width();
        let height = info.adjusted_height();
        // self.layer.set_size(width as u32, height as u32);
        self.egl_context.resize(&self.surface, width, height);
        // Resize the gl viewport
        self.egl_context.make_current().unwrap();
        self.renderer.resize().unwrap();
        self.surface.frame(qh, self.surface.clone());
    }

    pub fn change_size(&mut self, configure: LayerSurfaceConfigure, qh: &QueueHandle<Wpaperd>) {
        let mut info = self.info.borrow_mut();
        if info.change_size(configure) {
            drop(info);
            self.resize(qh);
        }
    }

    pub fn change_transform(&mut self, transform: Transform, qh: &QueueHandle<Wpaperd>) {
        let mut info = self.info.borrow_mut();
        if info.change_transform(transform) {
            drop(info);
            self.surface.set_buffer_transform(transform);
            self.resize(qh);
        }
    }

    pub fn change_scale_factor(&mut self, scale_factor: i32, qh: &QueueHandle<Wpaperd>) {
        let mut info = self.info.borrow_mut();
        if info.change_scale_factor(scale_factor) {
            drop(info);
            self.surface.set_buffer_scale(scale_factor);
            self.resize(qh);
        }
    }

    /// Check that the dimensions are valid
    pub fn is_configured(&self) -> bool {
        let info = self.info.borrow();
        info.width != 0 && info.height != 0
    }

    pub fn drawn(&self) -> bool {
        self.drawn
    }

    /// Update the wallpaper_info of this Surface
    /// return true if the duration has changed
    pub fn update_wallpaper_info(
        &mut self,
        handle: &LoopHandle<Wpaperd>,
        qh: &QueueHandle<Wpaperd>,
        mut wallpaper_info: WallpaperInfo,
    ) {
        if self.wallpaper_info != wallpaper_info {
            // Put the new value in place
            std::mem::swap(&mut self.wallpaper_info, &mut wallpaper_info);
            let path_changed = self.wallpaper_info.path != wallpaper_info.path;
            self.image_picker.update_sorting(
                self.wallpaper_info.sorting,
                path_changed,
                wallpaper_info.drawn_images_queue_size,
            );
            if path_changed {
                // ask the image_picker to pick a new a image
                self.image_picker.next_image();
            }
            if self.wallpaper_info.duration != wallpaper_info.duration {
                match (self.wallpaper_info.duration, wallpaper_info.duration) {
                    (None, None) => {
                        unreachable!()
                    }
                    // There was a duration before but now it has been removed
                    (None, Some(_)) => {
                        if let Some(registration_token) = self.event_source.take() {
                            handle.remove(registration_token);
                        }
                        if path_changed {
                            self.queue_draw(qh);
                        }
                    }
                    // There wasn't a duration before but now it has been added or it has changed
                    (Some(new_duration), None) | (Some(new_duration), Some(_)) => {
                        if let Some(registration_token) = self.event_source.take() {
                            handle.remove(registration_token);
                        }

                        // if the path has not changed or the duration has changed
                        // and the remaining time is great than 0
                        let timer = if let (false, Some(remaining_time)) = (
                            path_changed,
                            remaining_duration(
                                new_duration,
                                self.image_picker.image_changed_instant,
                            ),
                        ) {
                            Some(Timer::from_duration(remaining_time))
                        } else {
                            // otherwise draw the image immediately, the next timer
                            // will be set to the new duration
                            Some(Timer::immediate())
                        };

                        self.add_timer(timer, handle, qh.clone());
                    }
                }
            } else if self.wallpaper_info.mode != wallpaper_info.mode {
                if let Err(err) = self
                    .egl_context
                    .make_current()
                    .and_then(|_| self.renderer.set_mode(self.wallpaper_info.mode, false))
                {
                    error!("{err:?}");
                }
                self.queue_draw(qh);
            } else if self.wallpaper_info.drawn_images_queue_size
                != wallpaper_info.drawn_images_queue_size
            {
                self.image_picker
                    .update_queue_size(self.wallpaper_info.drawn_images_queue_size);
            } else if path_changed {
                self.queue_draw(qh);
            }
        }
    }

    /// Add a new timer in the event_loop for the current duration
    /// Stop if there is already a timer added
    pub fn add_timer(
        &mut self,
        timer: Option<Timer>,
        handle: &LoopHandle<Wpaperd>,
        qh: QueueHandle<Wpaperd>,
    ) {
        if let Some(duration) = self.wallpaper_info.duration {
            let timer = timer.unwrap_or(Timer::from_duration(duration));
            if self.event_source.is_some() {
                return;
            }

            let name = self.name().clone();
            let registration_token = handle
                .insert_source(
                    timer,
                    move |_deadline, _: &mut (), wpaperd: &mut Wpaperd| {
                        let surface = wpaperd
                            .surface_from_name(&name)
                            .with_context(|| format!("expecting surface {name} to be available"))
                            .unwrap();
                        if let Some(duration) = surface.wallpaper_info.duration {
                            // Check that the timer has expired
                            // if the daemon received a next or previous image command
                            // the timer will be reset and we need to account that here
                            // i.e. there is a timer of 1 minute. The user changes the image
                            // with a previous wallpaper command at 50 seconds.
                            // The timer will be reset to 1 minute and the image will be changed
                            if let Some(remaining_time) = remaining_duration(
                                duration,
                                surface.image_picker.image_changed_instant,
                            ) {
                                TimeoutAction::ToDuration(remaining_time)
                            } else {
                                // Change the drawn image
                                surface.image_picker.next_image();
                                surface.queue_draw(&qh);
                                TimeoutAction::ToDuration(duration)
                            }
                        } else {
                            TimeoutAction::Drop
                        }
                    },
                )
                .expect("Failed to insert event source!");

            self.event_source = Some(registration_token);
        }
    }

    pub fn is_drawing_animation(&self, time: u32) -> bool {
        self.renderer.is_drawing_animation(time)
    }

    pub(crate) fn queue_draw(&self, qh: &QueueHandle<Wpaperd>) {
        self.surface.frame(qh, self.surface.clone());
        self.surface.commit();
    }
}

fn black_image() -> RgbaImage {
    RgbaImage::from_raw(1, 1, vec![0, 0, 0, 255]).unwrap()
}

fn remaining_duration(duration: Duration, image_changed: Instant) -> Option<Duration> {
    // The timer has already expired
    let diff = image_changed.elapsed();
    if duration.saturating_sub(diff).is_zero() {
        None
    } else {
        Some(duration - diff)
    }
}
