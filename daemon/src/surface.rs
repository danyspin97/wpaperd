use std::sync::Arc;
use std::time::{Instant};

use color_eyre::Result;
use image::imageops::FilterType;
use image::{DynamicImage, ImageBuffer, Pixel, Rgba};
use smithay_client_toolkit::reexports::calloop::timer::{TimeoutAction, Timer};
use smithay_client_toolkit::reexports::calloop::{LoopHandle, RegistrationToken};
use smithay_client_toolkit::reexports::client::protocol::wl_output::WlOutput;
use smithay_client_toolkit::reexports::client::protocol::wl_surface;
use smithay_client_toolkit::shell::wlr_layer::{LayerSurface, LayerSurfaceConfigure};

use crate::image_picker::ImagePicker;
use crate::render::{EglContext, Renderer};
use crate::wallpaper_info::WallpaperInfo;
use crate::wpaperd::Wpaperd;

pub struct Surface {
    pub name: String,
    pub surface: wl_surface::WlSurface,
    pub output: WlOutput,
    pub layer: LayerSurface,
    pub dimensions: (u32, u32),
    pub scale: i32,
    egl_context: EglContext,
    renderer: Renderer,
    pub image_picker: ImagePicker,
    pub event_source: Option<RegistrationToken>,
    wallpaper_info: Arc<WallpaperInfo>,
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
        let egl_context = EglContext::new(egl_display, &surface);
        // Make the egl context as current to make the renderer creation work
        egl_context.make_current().unwrap();

        // Commit the surface
        surface.commit();

        Self {
            name,
            output,
            layer,
            dimensions: (0, 0),
            scale: scale_factor,
            surface,
            egl_context,
            renderer: unsafe { Renderer::new().unwrap() },
            image_picker: ImagePicker::new(wallpaper_info.clone()),
            event_source: None,
            wallpaper_info,
        }
    }

    /// Returns true if something has been drawn to the surface
    pub fn draw(&mut self) -> Result<()> {
        // No need to draw yet
        debug_assert!(self.dimensions.0 == 0 || self.dimensions.1 == 0);

        let width = self.dimensions.0 as i32 * self.scale;
        let height = self.dimensions.1 as i32 * self.scale;

        let image = self.image_picker.get_image()?;

        let mut image = image
            .resize_to_fill(width.try_into()?, height.try_into()?, FilterType::Lanczos3)
            .into_rgba8();

        self.apply_shadow(&mut image, width.try_into()?);

        self.egl_context.make_current()?;

        unsafe { self.renderer.draw(image.into())? };

        self.egl_context.swap_buffers()?;

        self.renderer.clear_after_draw()?;

        // Mark the entire surface as damaged
        self.surface.damage_buffer(0, 0, width, height);

        // Finally, commit the surface
        self.surface.commit();

        Ok(())
    }

    fn apply_shadow(&self, image: &mut ImageBuffer<Rgba<u8>, Vec<u8>>, width: u32) {
        if self.image_picker.apply_shadow() {
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

    pub fn name(&self) -> &str {
        &self.name
    }

    /// Resize the surface
    /// configure: None means that the scale factor has changed
    pub fn resize(&mut self, configure: Option<LayerSurfaceConfigure>) {
        self.dimensions = configure.map(|c| c.new_size).unwrap_or(self.dimensions);
        let width = TryInto::<i32>::try_into(self.dimensions.0).unwrap() * self.scale;
        let height = TryInto::<i32>::try_into(self.dimensions.1).unwrap() * self.scale;
        self.egl_context.resize(&self.surface, width, height);
        self.renderer.resize(width, height);

        // Draw the surface again
        self.draw().unwrap();
    }

    /// Check that the dimensions are valid
    pub(crate) fn is_configured(&self) -> bool {
        self.dimensions.0 != 0 && self.dimensions.1 != 0
    }

    /// Update the wallpaper_info of this Surface
    /// return true if the duration has changed
    pub fn update_wallpaper_info(
        &mut self,
        handle: LoopHandle<Wpaperd>,
        mut wallpaper_info: Arc<WallpaperInfo>,
    ) {
        if self.wallpaper_info != wallpaper_info {
            // Put the new value in place
            std::mem::swap(&mut self.wallpaper_info, &mut wallpaper_info);
            if self.wallpaper_info.duration != wallpaper_info.duration {
                match (self.wallpaper_info.duration, wallpaper_info.duration) {
                    (None, None) => {}
                    // There was a duration before but now it has been removed
                    (None, Some(_)) => {
                        if let Some(registration_token) = self.event_source.take() {
                            handle.remove(registration_token);
                        }
                    }
                    // There wasn't a duration before but now it has been added or it has changed
                    // This way, the timer started when the image was drawn, not when the duration
                    // has been set
                    (Some(new_duration), None) | (Some(new_duration), Some(_)) => {
                        if let Some(registration_token) = self.event_source.take() {
                            handle.remove(registration_token);
                        }
                        let now = Instant::now();
                        // The timer has already expired
                        let diff = now.duration_since(self.image_picker.image_changed_instant);
                        if new_duration.saturating_sub(diff).is_zero() {
                            self.add_timer(handle, Some(Timer::immediate()));
                        } else {
                            self.add_timer(handle, Some(Timer::from_duration(new_duration - diff)));
                        }
                    }
                }
            }
        }
    }

    /// Add a new timer in the event_loop for the current duration
    /// Stop if there is already a timer added
    pub fn add_timer(&mut self, handle: LoopHandle<Wpaperd>, timer: Option<Timer>) {
        if let Some(duration) = self.wallpaper_info.duration {
            let timer = timer.unwrap_or(Timer::from_duration(duration));
            if self.event_source.is_some() {
                return;
            }

            let name = self.name.clone();
            let registration_token = handle
                .insert_source(
                    timer,
                    move |_deadline, _: &mut (), wpaperd: &mut Wpaperd| {
                        // TODO: error handling
                        let surface = wpaperd.surface_from_name(&name).unwrap();
                        if let Some(duration) = surface.wallpaper_info.duration {
                            // Change the drawn image
                            surface.image_picker.next_image();
                            surface.draw().unwrap();

                            TimeoutAction::ToDuration(duration)
                        } else {
                            TimeoutAction::Drop
                        }
                    },
                )
                .expect("Failed to insert event source!");

            self.event_source = Some(registration_token);
        }
    }
}
