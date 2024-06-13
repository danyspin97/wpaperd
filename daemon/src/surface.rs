use std::{
    cell::RefCell,
    path::PathBuf,
    rc::Rc,
    time::{Duration, Instant},
};

use color_eyre::eyre::{Context, ContextCompat};
use color_eyre::Result;
use image::RgbaImage;
use log::{error, warn};
use smithay_client_toolkit::reexports::calloop::{LoopHandle, RegistrationToken};
use smithay_client_toolkit::reexports::client::protocol::wl_output::{Transform, WlOutput};
use smithay_client_toolkit::reexports::client::protocol::wl_surface;
use smithay_client_toolkit::reexports::client::QueueHandle;
use smithay_client_toolkit::shell::wlr_layer::{LayerSurface, LayerSurfaceConfigure};
use smithay_client_toolkit::{
    reexports::calloop::timer::{TimeoutAction, Timer},
    shell::WaylandSurface,
};

use crate::wpaperd::Wpaperd;
use crate::{display_info::DisplayInfo, wallpaper_info::WallpaperInfo};
use crate::{
    filelist_cache::FilelistCache,
    render::{EglContext, Renderer},
};
use crate::{image_loader::ImageLoader, image_picker::ImagePicker};

#[derive(Debug)]
pub enum EventSource {
    NotSet,
    Running(RegistrationToken),
    // The contained value is the duration that was left on the previous timer, used for starting the next timer.
    Paused(Duration),
}

pub struct Surface {
    pub surface: wl_surface::WlSurface,
    pub output: WlOutput,
    pub layer: LayerSurface,
    egl_context: EglContext,
    renderer: Renderer,
    pub image_picker: ImagePicker,
    pub event_source: EventSource,
    wallpaper_info: WallpaperInfo,
    info: Rc<RefCell<DisplayInfo>>,
    image_loader: Rc<RefCell<ImageLoader>>,
    drawn: bool,
    loading_image: Option<(PathBuf, usize)>,
    loading_image_tries: u8,
    /// Determines whether we should skip the next transition. Used to skip
    /// the first transition when starting up.
    ///
    /// See [crate::wallpaper_info::WallpaperInfo]'s `initial_transition` field
    skip_next_transition: bool,
    /// Pause state of the automatic wallpaper sequence.
    /// Setting this to true will mean only an explicit next/previous wallpaper command will change
    /// the wallpaper.
    should_pause: bool,
}

impl Surface {
    pub fn new(
        layer: LayerSurface,
        output: WlOutput,
        info: DisplayInfo,
        wallpaper_info: WallpaperInfo,
        egl_display: egl::Display,
        filelist_cache: Rc<RefCell<FilelistCache>>,
        image_loader: Rc<RefCell<ImageLoader>>,
    ) -> Self {
        let surface = layer.wl_surface().clone();
        let egl_context = EglContext::new(egl_display, &surface);
        // Make the egl context as current to make the renderer creation work
        egl_context
            .make_current()
            .expect("EGL context switching to work");

        // Commit the surface
        surface.commit();

        let image_picker = ImagePicker::new(&wallpaper_info, filelist_cache);

        let image = black_image();
        let info = Rc::new(RefCell::new(info));

        let renderer = unsafe {
            Renderer::new(
                image.into(),
                info.clone(),
                0,
                wallpaper_info.transition.clone(),
            )
            .expect("unable to create the renderer")
        };

        let first_transition = !wallpaper_info.initial_transition;
        let mut surface = Self {
            output,
            layer,
            info,
            surface,
            egl_context,
            renderer,
            image_picker,
            event_source: EventSource::NotSet,
            wallpaper_info,
            drawn: false,
            should_pause: false,
            image_loader,
            loading_image: None,
            loading_image_tries: 0,
            skip_next_transition: first_transition,
        };

        // Start loading the wallpaper as soon as possible (i.e. surface creation)
        // It will still be loaded as a texture when we have an openGL context
        if let Err(err) = surface.load_wallpaper(0) {
            warn!("{err:?}");
        }

        surface
    }

    /// Returns true if something has been drawn to the surface
    pub fn draw(&mut self, qh: &QueueHandle<Wpaperd>, time: u32) -> Result<()> {
        let info = self.info.borrow();
        let width = info.adjusted_width();
        let height = info.adjusted_height();
        // Drop the borrow to self
        drop(info);

        // Only returns true when the wallpaper is loaded
        if self.load_wallpaper(time)? || !self.drawn {
            // Use the correct context before loading the texture and drawing
            self.egl_context.make_current()?;

            let transition_going = unsafe { self.renderer.draw(time, self.wallpaper_info.mode)? };
            if transition_going {
                self.queue_draw(qh);
            } else {
                self.renderer.transition_finished();
            }

            self.drawn = true;

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
        } else {
            self.queue_draw(qh);
        }

        Ok(())
    }

    // Call surface::frame when this return false
    pub fn load_wallpaper(&mut self, time: u32) -> Result<bool> {
        Ok(loop {
            // If we were not already trying to load an image
            if self.loading_image.is_none() {
                if let Some(item) = self
                    .image_picker
                    .get_image_from_path(&self.wallpaper_info.path)
                {
                    // We are trying to load a new image
                    self.loading_image = Some(item);
                } else {
                    // we don't need to load any image
                    break true;
                }
            }
            let (image_path, index) = self
                .loading_image
                .as_ref()
                .expect("loading image to be set")
                .clone();
            let res = self
                .image_loader
                .borrow_mut()
                .background_load(image_path.to_owned(), self.name());
            match res {
                crate::image_loader::ImageLoaderStatus::Loaded(data) => {
                    // Renderer::load_wallpaper load the wallpaper in a openGL texture
                    // Set the correct opengl context
                    self.egl_context.make_current()?;
                    self.renderer
                        .load_wallpaper(data.into(), self.wallpaper_info.mode)?;

                    let transition_time = if self.skip_next_transition {
                        0
                    } else {
                        self.wallpaper_info.transition_time
                    };
                    self.skip_next_transition = false;

                    self.renderer.start_transition(time, transition_time);

                    if self.image_picker.is_reloading() {
                        self.image_picker.reloaded();
                    } else {
                        self.image_picker.update_current_image(image_path, index);
                    }
                    // Restart the counter
                    self.loading_image_tries = 0;
                    self.loading_image = None;
                    break true;
                }
                crate::image_loader::ImageLoaderStatus::Waiting => {
                    // wait until the image has been loaded
                    break false;
                }
                crate::image_loader::ImageLoaderStatus::Error => {
                    // We don't want to try too many times
                    self.loading_image_tries += 1;
                    // The image we were trying to load failed
                    self.loading_image = None;
                }
            }
            // If we have tried too many times, stop
            if self.loading_image_tries == 5 {
                break true;
            }
        })
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
        let display_name = self.name();
        let res = self
            .egl_context
            .resize(&self.surface, width, height)
            .with_context(|| {
                format!("unable to switch resize EGL context for display {display_name}",)
            })
            .and_then(|_| {
                self.egl_context.make_current().with_context(|| {
                    format!("unable to switch the openGL context for display {display_name}")
                })
            })
            .and_then(|_| {
                self.renderer.resize().with_context(|| {
                    format!("unable to resize the GL window for display {display_name}")
                })
            });
        // Resize the gl viewport
        if let Err(err) = res {
            error!("{err:?}");
        }
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
        if self.wallpaper_info == wallpaper_info {
            return;
        }

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
            self.queue_draw(qh);
        }
        if self.wallpaper_info.duration != wallpaper_info.duration {
            match (self.wallpaper_info.duration, wallpaper_info.duration) {
                (None, None) => {
                    unreachable!()
                }
                // There was a duration before but now it has been removed
                (None, Some(_)) => {
                    if let EventSource::Running(registration_token) = self.event_source {
                        handle.remove(registration_token);
                    }
                }
                // There wasn't a duration before but now it has been added or it has changed
                (Some(new_duration), None) | (Some(new_duration), Some(_)) => {
                    if let EventSource::Running(registration_token) = self.event_source {
                        handle.remove(registration_token);
                    }

                    // if the path has not changed or the duration has changed
                    // and the remaining time is great than 0
                    let timer = if let (false, Some(remaining_time)) = (
                        path_changed,
                        remaining_duration(new_duration, self.image_picker.image_changed_instant),
                    ) {
                        Some(Timer::from_duration(remaining_time))
                    } else {
                        // otherwise draw the image immediately, the next timer
                        // will be set to the new duration
                        Some(Timer::immediate())
                    };

                    self.event_source = EventSource::NotSet;
                    self.add_timer(timer, handle, qh.clone());
                }
            }
        }

        if self.wallpaper_info.mode != wallpaper_info.mode {
            if let Err(err) = self
                .egl_context
                .make_current()
                .and_then(|_| self.renderer.set_mode(self.wallpaper_info.mode, false))
            {
                error!("{err:?}");
            }
            if !path_changed {
                // We should draw immediately
                if let Err(err) = self.draw(qh, 0) {
                    warn!("{err:?}");
                }
            }
            if self.wallpaper_info.transition != wallpaper_info.transition {
                match self.egl_context.make_current() {
                    Ok(_) => {
                        self.renderer
                            .update_transition(self.wallpaper_info.transition.clone());
                    }
                    Err(err) => {
                        error!("{err:?}");
                    }
                }
            }
        }
        if self.wallpaper_info.drawn_images_queue_size != wallpaper_info.drawn_images_queue_size {
            self.image_picker
                .update_queue_size(self.wallpaper_info.drawn_images_queue_size);
        }
        if self.wallpaper_info.transition_time != wallpaper_info.transition_time {
            self.renderer
                .update_transition_time(self.wallpaper_info.transition_time);
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
        if matches!(self.event_source, EventSource::Running(_)) {
            return;
        }
        let Some(duration) = self.wallpaper_info.duration else {
            return;
        };

        let timer = timer.unwrap_or(Timer::from_duration(duration));

        let name = self.name().clone();
        let registration_token = handle
            .insert_source(
                timer,
                move |_deadline, _: &mut (), wpaperd: &mut Wpaperd| {
                    let surface = match wpaperd
                        .surface_from_name(&name)
                        .with_context(|| format!("expecting surface {name} to be available"))
                    {
                        Ok(surface) => surface,
                        Err(err) => {
                            error!("{err:?}");
                            return TimeoutAction::Drop;
                        }
                    };

                    if let Some(duration) = surface.wallpaper_info.duration {
                        // Check that the timer has expired
                        // if the daemon received a next or previous image command
                        // the timer will be reset and we need to account that here
                        // i.e. there is a timer of 1 minute. The user changes the image
                        // with a previous wallpaper command at 50 seconds.
                        // The timer will be reset to 1 minute and the image will be changed
                        if let Some(remaining_time) =
                            remaining_duration(duration, surface.image_picker.image_changed_instant)
                        {
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

        self.event_source = EventSource::Running(registration_token);
    }

    /// Handle updating the timer based on the pause state of the automatic wallpaper sequence.
    /// Remove the timer if pausing, and add a new timer with the remaining duration of the old
    /// timer when resuming.
    pub fn handle_pause_state(&mut self, handle: &LoopHandle<Wpaperd>, qh: QueueHandle<Wpaperd>) {
        match (self.should_pause, &self.event_source) {
            // Should pause, but timer is still currently running
            (true, EventSource::Running(registration_token)) => {
                let remaining_duration = self.get_remaining_duration().unwrap_or_default();

                handle.remove(*registration_token);
                self.event_source = EventSource::Paused(remaining_duration);
            }
            // Should resume, but timer is not currently running
            (false, EventSource::Paused(duration)) => {
                self.add_timer(Some(Timer::from_duration(*duration)), handle, qh.clone());
            }
            // Otherwise no update is necessary
            (_, _) => {}
        }
    }

    #[inline]
    pub fn queue_draw(&mut self, qh: &QueueHandle<Wpaperd>) {
        // Start loading the next image immediately
        if let Err(err) = self.load_wallpaper(0) {
            warn!("{err:?}");
        }
        self.surface.frame(qh, self.surface.clone());
        self.surface.commit();
    }

    #[inline]
    fn get_remaining_duration(&self) -> Option<Duration> {
        let duration = self.wallpaper_info.duration?;
        remaining_duration(duration, self.image_picker.image_changed_instant)
    }

    /// Indicate to the main event loop that the automatic wallpaper sequence for this [`Surface`]
    /// should be paused.
    /// The actual pausing/resuming is handled in [`Surface::handle_pause_state`]
    #[inline]
    pub fn pause(&mut self) {
        self.should_pause = true;
    }
    /// Indicate to the main event loop that the automatic wallpaper sequence for this [`Surface`]
    /// should be resumed.
    /// The actual pausing/resuming is handled in [`Surface::handle_pause_state`]
    #[inline]
    pub fn resume(&mut self) {
        self.should_pause = false;
    }

    /// Returns a boolean representing whether this [`Surface`] is set to indicate to the main event
    /// loop that its automatic wallpaper sequence should be paused.
    #[inline]
    pub fn should_pause(&self) -> bool {
        self.should_pause
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
