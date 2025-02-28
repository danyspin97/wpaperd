use std::{
    cell::RefCell,
    fs,
    ops::Add,
    path::{Path, PathBuf},
    rc::Rc,
    time::{Duration, Instant},
};

use color_eyre::{
    eyre::{OptionExt, WrapErr},
    Result,
};
use image::RgbaImage;
use log::{error, warn};
use smithay_client_toolkit::{
    reexports::{
        calloop::{
            timer::{TimeoutAction, Timer},
            LoopHandle, RegistrationToken,
        },
        client::{
            protocol::{
                wl_output::{Transform, WlOutput},
                wl_surface,
            },
            QueueHandle,
        },
    },
    shell::{
        wlr_layer::{LayerSurface, LayerSurfaceConfigure},
        WaylandSurface,
    },
};

use crate::{
    display_info::DisplayInfo,
    image_loader::ImageLoader,
    image_picker::ImagePicker,
    render::{EglContext, Renderer},
    wallpaper_groups::WallpaperGroups,
    wallpaper_info::WallpaperInfo,
    wpaperd::Wpaperd,
};

#[derive(Debug)]
pub enum EventSource {
    NotSet,
    /// We need the registration token to remove the timer,
    /// the duration to know how much time this timer is waiting for
    /// and the instant when the image was changed to calculate the remaining
    Running(RegistrationToken, Duration, Instant),
    // The contained value is the duration that was left on the previous timer, used for starting the next timer.
    Paused(Duration),
}

pub struct Surface {
    wl_surface: wl_surface::WlSurface,
    wl_output: WlOutput,
    layer: LayerSurface,
    egl_context: EglContext,
    renderer: Renderer,
    pub image_picker: ImagePicker,
    event_source: EventSource,
    pub wallpaper_info: WallpaperInfo,
    info: Rc<RefCell<DisplayInfo>>,
    image_loader: Rc<RefCell<ImageLoader>>,
    window_drawn: bool,
    pub loading_image: Option<(PathBuf, usize)>,
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
    /// Contains the value of XDG_STATE_HOME, given by wapaperd at struct creation
    xdg_state_home: PathBuf,
}

impl Surface {
    pub fn new(
        wpaperd: &Wpaperd,
        wl_layer: LayerSurface,
        wl_output: WlOutput,
        info: DisplayInfo,
        wallpaper_info: WallpaperInfo,
        xdg_state_home: PathBuf,
    ) -> Result<Self> {
        let wl_surface = wl_layer.wl_surface().clone();
        let egl_context =
            EglContext::new(wpaperd.egl_display, &wl_surface).wrap_err_with(|| {
                format!("Failed to initialize EGL context for display {}", info.name)
            })?;
        // Make the egl context as current to make the renderer creation work
        egl_context
            .make_current()
            .wrap_err("Failed to switch EGL context")?;

        // Commit the surface
        wl_surface.commit();

        let image_picker = ImagePicker::new(
            &wallpaper_info,
            &wl_surface,
            wpaperd.filelist_cache.clone(),
            wpaperd.wallpaper_groups.clone(),
        );

        let image = black_image();
        let info = Rc::new(RefCell::new(info));

        let renderer = unsafe {
            Renderer::new(
                image.into(),
                info.clone(),
                0,
                wallpaper_info.transition.clone(),
                info.borrow().transform,
            )
            .wrap_err("Failed to create the renderer")?
        };

        let first_transition = !wallpaper_info.initial_transition;
        let mut surface = Self {
            wl_output,
            layer: wl_layer,
            info,
            wl_surface,
            egl_context,
            renderer,
            image_picker,
            event_source: EventSource::NotSet,
            wallpaper_info,
            window_drawn: false,
            should_pause: false,
            image_loader: wpaperd.image_loader.clone(),
            loading_image: None,
            loading_image_tries: 0,
            skip_next_transition: first_transition,
            xdg_state_home,
        };

        // Start loading the wallpaper as soon as possible (i.e. surface creation)
        // It will still be loaded as a texture when we have an openGL context
        if let Err(err) = surface.load_wallpaper(None) {
            warn!(
                "{:?}",
                err.wrap_err(format!(
                    "Failed to start loading the wallpaper in background for display {}",
                    surface.info.borrow().name
                ))
            );
        }

        Ok(surface)
    }

    /// Returns true if something has been drawn to the surface
    pub fn draw(&mut self, qh: &QueueHandle<Wpaperd>, time: Option<u32>) -> Result<()> {
        let info = self.info.borrow();
        let width = info.adjusted_width();
        let height = info.adjusted_height();
        // Drop the borrow to self
        drop(info);

        // Use the correct context before drawing
        self.egl_context
            .make_current()
            .wrap_err("Failed to switch EGL context")?;

        if self.renderer.transition_running() {
            // Recalculate the current progress, the transition might end now
            let transition_running = self.renderer.update_transition_status(time.unwrap_or(0));
            // If we don't have any time passed, just consider the transition to be ended
            if transition_running {
                // Don't call queue_draw as it calls load_wallpaper again
                self.wl_surface.frame(qh, self.wl_surface.clone());
            } else {
                self.renderer.transition_finished();
            }
            // We are waiting for an image to be loaded in memory
        } else if self.loading_image.is_some() {
            self.wl_surface.frame(qh, self.wl_surface.clone());
            // We need to draw the first time, do not exit this function
            if self.window_drawn {
                // We need to call commit, otherwise the call to frame above doesn't work
                self.wl_surface().commit();
                return Ok(());
            }
        }

        unsafe { self.renderer.draw()? }

        self.renderer
            .clear_after_draw()
            .wrap_err("Failed to unbind the buffer")?;
        self.egl_context
            .swap_buffers()
            .wrap_err("Failed to swap EGL buffers")?;

        // Reset the context
        egl::API
            .make_current(self.egl_context.display, None, None, None)
            .wrap_err("Failed to reset the EGL context")?;

        // Mark the entire surface as damaged
        self.wl_surface.damage_buffer(0, 0, width, height);

        // Finally, commit the surface
        self.wl_surface.commit();

        Ok(())
    }

    pub fn try_drawing(&mut self, qh: &QueueHandle<Wpaperd>, time: Option<u32>) -> bool {
        match self.draw(qh, time) {
            Ok(_) => true,
            Err(err) => {
                error!(
                    "{:?}",
                    err.wrap_err(format!(
                        "Failed to draw on display {}",
                        self.info.borrow().name
                    ))
                );
                false
            }
        }
    }

    // Start loading a wallpaper with the image_loader.
    // Returns true when it is loaded, false when we need to wait
    // Call surface::frame when this return false
    pub fn load_wallpaper(&mut self, handle: Option<&LoopHandle<Wpaperd>>) -> Result<bool> {
        // If we were not already trying to load an image
        if self.loading_image.is_none() {
            if let Some(item) = self.image_picker.get_image_from_path(
                &self.wallpaper_info.path,
                &self.wallpaper_info.recursive.clone(),
            ) {
                if self.image_picker.current_image() == item.0 && !self.image_picker.is_reloading()
                {
                    return Ok(true);
                }
                self.loading_image = Some(item);
            } else {
                // we don't need to load any image
                return Ok(true);
            }
        }

        let (image_path, index) = self
            .loading_image
            .as_ref()
            .expect("loading image to be set")
            .clone();

        if self.renderer.transition_running() {
            return Ok(true);
        }

        let res = self
            .image_loader
            .borrow_mut()
            .background_load(image_path.to_owned(), self.name());
        match res {
            crate::image_loader::ImageLoaderStatus::Loaded(data) => {
                // Renderer::load_wallpaper load the wallpaper in a openGL texture
                // Set the correct opengl context
                self.egl_context
                    .make_current()
                    .wrap_err("Failed to switch EGL context")?;
                self.renderer.load_wallpaper(
                    data.into(),
                    self.wallpaper_info.mode,
                    self.wallpaper_info.offset,
                )?;

                if self.image_picker.is_reloading() {
                    self.image_picker.reloaded();
                } else if let Some(handle) = handle {
                    self.setup_drawing_image(image_path, index, handle);
                } else {
                    warn!(
                        "No handle to add transition timer for display {}",
                        self.info.borrow().name
                    );
                }
                // Restart the counter
                self.loading_image_tries = 0;
                self.loading_image = None;
                Ok(true)
            }
            crate::image_loader::ImageLoaderStatus::Waiting => {
                // wait until the image has been loaded
                Ok(false)
            }
            crate::image_loader::ImageLoaderStatus::Error => {
                // We don't want to try too many times
                self.loading_image_tries += 1;
                // The image we were trying to load failed
                self.loading_image = None;
                // If we have tried too many times, stop
                if self.loading_image_tries != 5 {
                    return self.load_wallpaper(handle);
                }
                Ok(false)
            }
        }
    }

    pub fn setup_drawing_image(
        &mut self,
        image_path: PathBuf,
        index: usize,
        handle: &LoopHandle<Wpaperd>,
    ) {
        let transition_time = if self.skip_next_transition {
            0
        } else {
            self.wallpaper_info.transition_time
        };
        self.skip_next_transition = false;

        self.update_wallpaper_link(&image_path);
        self.image_picker.update_current_image(image_path, index);
        self.renderer.start_transition(transition_time);
        self.add_transition_timer(handle);
        // Update the instant where we have drawn the image
        if let EventSource::Running(registration_token, duration, _) = self.event_source {
            self.event_source = EventSource::Running(registration_token, duration, Instant::now());
        }
    }

    pub fn add_transition_timer(&mut self, handle: &LoopHandle<Wpaperd>) {
        let timer =
            Timer::from_duration(Duration::from_millis(self.renderer.transition_time.into()));

        let name = self.name().clone();
        if let Err(err) = handle.insert_source(
            timer,
            move |_deadline, _: &mut (), wpaperd: &mut Wpaperd| {
                let surface = match wpaperd.surface_from_name(&name).ok_or_eyre(format!(
                    "Surface for display {name} is not available available in wpaperd registry"
                )) {
                    Ok(surface) => surface,
                    Err(err) => {
                        error!("{err:?}");
                        return TimeoutAction::Drop;
                    }
                };

                if let EventSource::Running(_, _, instant) = surface.event_source {
                    // if the time we are drawing is past the transition_time
                    let time_left = Duration::from_millis(surface.renderer.transition_time.into())
                        .saturating_sub(instant.elapsed());
                    if time_left.is_zero() {
                        // skip the transition and draw the image directly
                        surface.renderer.transition_finished();
                        TimeoutAction::Drop
                    } else {
                        TimeoutAction::ToDuration(time_left)
                    }
                } else {
                    TimeoutAction::Drop
                }
            },
        ) {
            error!("{err:?}");
        }
    }

    pub fn name(&self) -> String {
        self.info.borrow().name.to_string()
    }

    pub fn description(&self) -> String {
        self.info.borrow().description.to_string()
    }

    /// Resize the surface
    pub fn resize(&mut self, qh: &QueueHandle<Wpaperd>) -> Result<()> {
        let info = self.info.borrow();
        let width = info.adjusted_width();
        let height = info.adjusted_height();
        // Drop the borrow to self
        drop(info);
        // self.layer.set_size(width as u32, height as u32);
        self.egl_context
            .resize(&self.wl_surface, width, height)
            .wrap_err("Failed to resize EGL window")?;
        self.egl_context
            .make_current()
            .wrap_err("Failed to switch to the EGL context")?;
        self.renderer
            .resize()
            .wrap_err("Failed to resize GL window")?;
        // If we resize, stop immediately any lingering transition
        self.renderer.force_transition_end();

        // Queue drawing for the next frame. We can directly draw here, but we would still
        // need to queue the draw for the next frame, otherwise wpaperd doesn't work at startup
        self.queue_draw(qh);

        Ok(())
    }

    pub fn change_size(&mut self, configure: LayerSurfaceConfigure, qh: &QueueHandle<Wpaperd>) {
        let mut info = self.info.borrow_mut();
        if info.change_size(configure) {
            drop(info);
            if let Err(err) = self
                .resize(qh)
                .wrap_err_with(|| {
                    format!(
                        "Failed to resize the surface for display {}",
                        self.info.borrow().name
                    )
                })
                .and_then(|_| {
                    self.renderer
                        .set_mode(self.wallpaper_info.mode, self.wallpaper_info.offset)
                        .wrap_err("Failed to change wallpaper mode")
                })
            {
                error!("{err:?}");
            }
        }
    }

    pub fn change_transform(&mut self, transform: Transform, qh: &QueueHandle<Wpaperd>) {
        let mut info = self.info.borrow_mut();
        if info.change_transform(transform) {
            drop(info);
            self.wl_surface.set_buffer_transform(transform);
            if let Err(err) = self
                .resize(qh)
                .wrap_err("Failed to resize the surface")
                .and_then(|_| {
                    self.renderer
                        .set_mode(self.wallpaper_info.mode, self.wallpaper_info.offset)
                        .wrap_err("Failed to change wallpaper mode")
                })
                .and_then(|_| unsafe {
                    self.renderer
                        .set_projection_matrix(transform)
                        .wrap_err("Failed to change wallpaper mode")
                })
                .wrap_err_with(|| {
                    format!(
                        "Failed to change transform for display {}",
                        self.info.borrow().name
                    )
                })
            {
                error!("{err:?}");
            }
        }
    }

    pub fn change_scale_factor(&mut self, scale_factor: i32, qh: &QueueHandle<Wpaperd>) {
        let mut info = self.info.borrow_mut();
        if info.change_scale_factor(scale_factor) {
            drop(info);
            self.wl_surface.set_buffer_scale(scale_factor);
            // Resize the gl viewport
            if let Err(err) = self.resize(qh).wrap_err_with(|| {
                format!(
                    "Failed to resize the surface for display {}",
                    self.info.borrow().name
                )
            }) {
                error!("{err:?}");
            }
        }
    }

    /// Check that the dimensions are valid
    pub fn is_configured(&self) -> bool {
        let info = self.info.borrow();
        info.width != 0 && info.height != 0
    }

    pub fn has_been_drawn(&self) -> bool {
        self.window_drawn
    }

    pub fn drawn(&mut self) {
        self.window_drawn = true;
    }

    /// Update the wallpaper_info of this Surface
    /// return true if the duration has changed
    pub fn update_wallpaper_info(
        &mut self,
        handle: &LoopHandle<Wpaperd>,
        qh: &QueueHandle<Wpaperd>,
        mut wallpaper_info: WallpaperInfo,
        wallpaper_groups: Rc<RefCell<WallpaperGroups>>,
    ) {
        if self.wallpaper_info == wallpaper_info {
            return;
        }

        // Put the new value in place
        std::mem::swap(&mut self.wallpaper_info, &mut wallpaper_info);
        // if the two paths are different and the new path is a directory but doesn't contain the
        // old image
        let path_changed = self.wallpaper_info.path != wallpaper_info.path
            && self.wallpaper_info.path.is_dir()
                && !wallpaper_info.path.starts_with(&self.wallpaper_info.path)
            // and the recursive mode is different
            && wallpaper_info.recursive.as_ref().zip(self.wallpaper_info.recursive.as_ref()).map(|(x, y)| x != y).unwrap_or(false);
        self.image_picker.update_sorting(
            self.wallpaper_info.sorting,
            &self.wallpaper_info.path,
            self.wallpaper_info.recursive,
            path_changed,
            &self.wl_surface,
            wallpaper_info.drawn_images_queue_size,
            &wallpaper_groups,
        );
        if path_changed {
            // ask the image_picker to pick a new a image
            self.image_picker
                .next_image(&self.wallpaper_info.path, &self.wallpaper_info.recursive);
        }
        // Always queue draw to load changes (needed for GroupedRandom)
        self.queue_draw(qh);
        self.handle_new_duration(&wallpaper_info, handle, path_changed, qh);

        if self.wallpaper_info.mode != wallpaper_info.mode
            || self.wallpaper_info.offset != wallpaper_info.offset
        {
            if let Err(err) = self.egl_context.make_current().and_then(|_| {
                self.renderer
                    .set_mode(self.wallpaper_info.mode, self.wallpaper_info.offset)
                    .wrap_err_with(|| {
                        format!(
                            "Failed to change wallpaper mode for display {}",
                            self.info.borrow().name
                        )
                    })
            }) {
                error!("{err:?}");
            }
            if !path_changed {
                // We should draw immediately
                self.try_drawing(qh, None);
            }
        }
        if self.wallpaper_info.transition != wallpaper_info.transition {
            match self.egl_context.make_current().wrap_err_with(|| {
                format!(
                    "Failed to switch EGL context for display {}",
                    self.info.borrow().name
                )
            }) {
                Ok(_) => {
                    let transform = self.renderer.display_info.borrow().transform;
                    self.renderer
                        .update_transition(self.wallpaper_info.transition.clone(), transform);
                }
                Err(err) => {
                    error!("{err:?}");
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

    fn handle_new_duration(
        &mut self,
        wallpaper_info: &WallpaperInfo,
        handle: &LoopHandle<Wpaperd>,
        path_changed: bool,
        qh: &QueueHandle<Wpaperd>,
    ) {
        if self.wallpaper_info.duration != wallpaper_info.duration {
            match (self.wallpaper_info.duration, wallpaper_info.duration) {
                (None, None) => {
                    unreachable!()
                }
                // There was a duration before but now it has been removed
                (None, Some(_)) => {
                    if let EventSource::Running(registration_token, _, _) = self.event_source {
                        handle.remove(registration_token);
                    }
                    self.event_source = EventSource::NotSet;
                }
                // There wasn't a duration before but now it has been added or it has changed
                (Some(new_duration), Some(old_duration)) => {
                    let duration = if !path_changed {
                        // The image drawn is still the same, calculate the time
                        // it was on screen without the timer being paused
                        let time_passed = match self.event_source {
                            EventSource::Running(_, duration, instant) => {
                                // The old_duration is the full duration that the wallpaper needed
                                // to be displayed. The duration is the one that the timer is set
                                // to, which might be different than old_duration if the timer was
                                // paused. So calculate how much time the image was displayed with
                                // this information.
                                old_duration.saturating_sub(duration) + instant.elapsed()
                            }
                            EventSource::Paused(duration) => old_duration - duration,
                            EventSource::NotSet => unreachable!(),
                        };

                        let saturating_sub = new_duration.saturating_sub(time_passed);
                        if saturating_sub.is_zero() {
                            // The image was on screen for the same time as the new duration
                            self.image_picker.next_image(
                                &self.wallpaper_info.path,
                                &self.wallpaper_info.recursive,
                            );
                            if let Err(err) = self.load_wallpaper(None).wrap_err_with(|| {
                                format!(
                                    "Failed to query the image loader for display {}",
                                    self.info.borrow().name
                                )
                            }) {
                                warn!("{err:?}");
                            }
                            new_duration
                        } else {
                            saturating_sub
                        }
                    } else {
                        // the path_changed, we drew a new image, restart the timer
                        new_duration
                    };
                    match self.event_source {
                        EventSource::Running(registration_token, _, _) => {
                            // Remove the previous timer and add a new one
                            handle.remove(registration_token);
                            self.event_source = EventSource::NotSet;
                            self.add_timer(handle, qh.clone(), Some(duration));
                        }
                        EventSource::Paused(_) => {
                            // Add a new paused timer
                            self.event_source = EventSource::Paused(duration);
                        }
                        EventSource::NotSet => unreachable!(),
                    }
                }
                _ => {
                    self.add_timer(
                        handle,
                        qh.clone(),
                        // The new duration will be picked by add_timer
                        None,
                    );
                }
            }
        }
    }

    /// Add a new timer in the event_loop for the current duration
    /// Stop if there is already a timer added
    pub fn add_timer(
        &mut self,
        handle: &LoopHandle<Wpaperd>,
        qh: QueueHandle<Wpaperd>,
        duration_left: Option<Duration>,
    ) {
        // Timer is already running
        if matches!(self.event_source, EventSource::Running(_, _, _)) {
            return;
        }
        // We need a duration to set a timer
        let duration = match duration_left {
            Some(duration) => Some(duration),
            // Add the transition time to have more precise duration
            None => self.wallpaper_info.duration.map(|d| {
                d.add(Duration::from_millis(
                    self.wallpaper_info.transition_time.into(),
                ))
            }),
        };
        let Some(duration) = duration else { return };

        let timer = Timer::from_duration(duration);

        let name = self.name().clone();
        let registration_token = handle
            .insert_source(
                timer,
                move |_deadline, _: &mut (), wpaperd: &mut Wpaperd| {
                    let surface = match wpaperd.surface_from_name(&name).ok_or_eyre({
                        format!("Surface for display {name} is not available in wpaperd registry")
                    }) {
                        Ok(surface) => surface,
                        Err(err) => {
                            error!("{err:?}");
                            return TimeoutAction::Drop;
                        }
                    };

                    // get duration from self.event_source
                    match surface.event_source {
                        EventSource::Running(_, _, _)
                            if surface.wallpaper_info.duration.is_none() =>
                        {
                            TimeoutAction::Drop
                        }
                        EventSource::Running(registration_token, duration, instant) => {
                            // The timer went off before the actual duration expired, run the next
                            // one with the remaining duration
                            let duration = if let Some(duration_left) =
                                remaining_duration(duration, instant)
                            {
                                duration_left
                            } else {
                                // otherwise get the next image and set the new duration
                                // before doing so, we need to check that the transition ended
                                // if it didn't, it means that the transition never ran.
                                // It happens when there is a display with a fullscreen window
                                // and wpaperd surface doesn't receive any frame event.
                                if surface.renderer.transition_running() {
                                    // Mark the transition ended, so that we have simulated the
                                    // entire drawing of an image
                                    // This actually never gets called if the draw function can end
                                    // the transition itself. Still, this might be triggered with
                                    // other compositors, left as a safety measure.
                                    surface.renderer.transition_finished();
                                    surface.renderer.force_transition_end();
                                }
                                surface.image_picker.next_image(
                                    &surface.wallpaper_info.path,
                                    &surface.wallpaper_info.recursive,
                                );
                                surface.queue_draw(&qh);
                                surface.wallpaper_info.duration.unwrap()
                            };
                            surface.event_source =
                                EventSource::Running(registration_token, duration, Instant::now());
                            TimeoutAction::ToDuration(duration)
                        }
                        EventSource::NotSet => TimeoutAction::Drop,
                        _ => unreachable!("timer must be running"),
                    }
                },
            )
            .expect("Failed to insert event source!");

        self.event_source = EventSource::Running(registration_token, duration, Instant::now());
    }

    /// Handle updating the timer based on the pause state of the automatic wallpaper sequence.
    /// Remove the timer if pausing, and add a new timer with the remaining duration of the old
    /// timer when resuming.
    pub fn handle_pause_state(&mut self, handle: &LoopHandle<Wpaperd>, qh: QueueHandle<Wpaperd>) {
        match (self.should_pause, &self.event_source) {
            // Should pause, but timer is still currently running
            (true, EventSource::Running(registration_token, duration, instant)) => {
                let remaining_duration = remaining_duration(*duration, *instant);

                handle.remove(*registration_token);
                // The remaining duration should never be 0
                self.event_source = EventSource::Paused(
                    remaining_duration.expect("timer must have already been expired"),
                );
            }
            // Should resume, but timer is not currently running
            (false, EventSource::Paused(duration)) => {
                self.add_timer(handle, qh.clone(), Some(*duration));
            }
            // Otherwise no update is necessary
            (_, _) => {}
        }
    }

    #[inline]
    pub fn queue_draw(&mut self, qh: &QueueHandle<Wpaperd>) {
        if let Err(err) = self.load_wallpaper(None).wrap_err_with(|| {
            format!(
                "Failed to query the image loader for display {}",
                self.info.borrow().name
            )
        }) {
            warn!("{err:?}");
        }
        self.wl_surface.frame(qh, self.wl_surface.clone());
        self.wl_surface.commit();
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

    /// Toggle the pause state for this [`Surface`], which is responsible for indicating to the main
    /// event loop that the automatic wallpaper sequence should be paused.
    /// The actual pausing/resuming is handled in [`Surface::handle_pause_state`]
    #[inline]
    pub fn toggle_pause(&mut self) {
        if self.should_pause() {
            self.resume();
        } else {
            self.pause();
        };
    }

    /// Returns a boolean representing whether this [`Surface`] is set to indicate to the main event
    /// loop that its automatic wallpaper sequence should be paused.
    #[inline]
    pub fn should_pause(&self) -> bool {
        self.should_pause
    }

    pub fn wl_surface(&self) -> &wl_surface::WlSurface {
        &self.wl_surface
    }

    pub fn wl_output(&self) -> &WlOutput {
        &self.wl_output
    }

    pub fn layer(&self) -> &LayerSurface {
        &self.layer
    }

    pub fn status(&self) -> &'static str {
        if self.wallpaper_info.path.is_dir() {
            if self.should_pause {
                "paused"
            } else {
                "running"
            }
        } else {
            "static"
        }
    }

    pub fn get_remaining_duration(&self) -> Option<Duration> {
        match &self.event_source {
            EventSource::Running(_, duration, instant) => remaining_duration(*duration, *instant),
            EventSource::Paused(duration) => Some(*duration),
            EventSource::NotSet => None,
        }
    }

    /// Add a symlink into .local/state that points to the current wallpaper
    fn update_wallpaper_link(&self, image_path: &Path) {
        let link = self.xdg_state_home.join(&self.info.borrow().name);
        // remove the previous file if it exists, otherwise symlink() fails
        if link.exists() {
            if let Err(err) = fs::remove_file(&link)
                .wrap_err_with(|| format!("Failed to remove symlink {link:?}"))
            {
                warn!("{err:?}");
                // Do no try to create a new symlink
                return;
            }
        }
        if let Err(err) = std::os::unix::fs::symlink(image_path, &link)
            .wrap_err_with(|| format!("Failed to create symlink {link:?} to {image_path:?}"))
        {
            warn!("{err:?}");
        }
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        // Do not leave any symlink when a surface gets destroyed
        let link = self.xdg_state_home.join(&self.info.borrow().name);
        if link.exists() {
            if let Err(err) = fs::remove_file(&link)
                .wrap_err_with(|| format!("Failed to remove symlink {link:?}"))
            {
                warn!("{err:?}");
            }
        }
    }
}

fn black_image() -> RgbaImage {
    RgbaImage::from_raw(1, 1, vec![0, 0, 0, 255]).unwrap()
}

fn remaining_duration(duration: Duration, image_changed: Instant) -> Option<Duration> {
    let diff = image_changed.elapsed();

    // only use seconds, we don't need to be precise
    let duration = Duration::from_secs(duration.as_secs());
    let diff = Duration::from_secs(diff.as_secs());

    if duration.saturating_sub(diff).is_zero() {
        None
    } else {
        Some(duration - diff)
    }
}
