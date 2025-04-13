use image::DynamicImage;
use log::warn;
use smithay_client_toolkit::reexports::client::{protocol::wl_surface::WlSurface, Proxy};
use wayland_egl::WlEglSurface;

use egl::API as egl;

use color_eyre::{
    eyre::{eyre, Context, OptionExt},
    Result,
};

use crate::{
    display_info::DisplayInfo,
    wallpaper_info::{BackgroundMode, WallpaperInfo},
};

use super::Renderer;

pub struct EglContext {
    display: egl::Display,
    context: egl::Context,
    config: egl::Config,
    wl_egl_surface: WlEglSurface,
    surface: khronos_egl::Surface,
    display_name: String,
    pub renderer: Renderer,
}

impl EglContext {
    pub fn new(
        egl_display: egl::Display,
        wl_surface: &WlSurface,
        wallpaper_info: &WallpaperInfo,
        display_info: &DisplayInfo,
    ) -> Result<Self> {
        const ATTRIBUTES: [i32; 7] = [
            egl::RED_SIZE,
            8,
            egl::GREEN_SIZE,
            8,
            egl::BLUE_SIZE,
            8,
            egl::NONE,
        ];

        let config = egl
            .choose_first_config(egl_display, &ATTRIBUTES)
            .wrap_err("Failed to find EGL configurations")?
            .ok_or_eyre("No available EGL configuration")?;

        const CONTEXT_ATTRIBUTES: [i32; 5] = [
            egl::CONTEXT_MAJOR_VERSION,
            2,
            egl::CONTEXT_MINOR_VERSION,
            0,
            egl::NONE,
        ];

        let context = egl
            .create_context(egl_display, config, None, &CONTEXT_ATTRIBUTES)
            .wrap_err("Failed to create an EGL context")?;

        // First, create a small surface, we don't know the size of the output yet
        let wl_egl_surface = WlEglSurface::new(wl_surface.id(), 10, 10)
            .wrap_err("Failed to create a WlEglSurface")?;

        let surface = unsafe {
            egl.create_window_surface(
                egl_display,
                config,
                wl_egl_surface.ptr() as egl::NativeWindowType,
                None,
            )
            .wrap_err("Failed to create an EGL window surface")?
        };

        // Make the egl context as current to make the renderer creation work
        egl.make_current(egl_display, Some(surface), Some(surface), Some(context))
            .wrap_err("Failed to set the current EGL context")?;

        let renderer = unsafe {
            Renderer::new(
                wallpaper_info.transition_time,
                wallpaper_info.transition.clone(),
                display_info,
            )
            .wrap_err("Failed to create a openGL ES renderer")?
        };

        Ok(Self {
            display: egl_display,
            context,
            config,
            surface,
            wl_egl_surface,
            display_name: display_info.name.to_owned(),
            renderer,
        })
    }

    #[inline]
    pub fn make_current(&self) -> Result<()> {
        egl.make_current(
            self.display,
            Some(self.surface),
            Some(self.surface),
            Some(self.context),
        )
        .wrap_err("Failed to set the current EGL context")?;

        egl.swap_interval(self.display, 0)
            .wrap_err("Failed to disable vsync for the EGL context")
    }

    // Swap the buffers of the surface
    #[inline]
    pub fn swap_buffers(&self) -> Result<()> {
        egl.swap_buffers(self.display, self.surface)
            .wrap_err("Failed to draw the content of the GL buffer")
    }

    /// Resize the surface
    /// Resizing the surface means to destroy the previous one and then recreate it
    pub fn resize(&mut self, wl_surface: &WlSurface, display_info: &DisplayInfo) -> Result<()> {
        egl.destroy_surface(self.display, self.surface)
            .wrap_err("Failed to destroy the EGL surface")?;
        let wl_egl_surface = WlEglSurface::new(
            wl_surface.id(),
            display_info.adjusted_width(),
            display_info.adjusted_height(),
        )
        .wrap_err("Failed to create a WlEglSurface")?;

        let surface = unsafe {
            egl.create_window_surface(
                self.display,
                self.config,
                wl_egl_surface.ptr() as egl::NativeWindowType,
                None,
            )
            .wrap_err("Failed to create an EGL window surface")?
        };

        self.surface = surface;
        self.wl_egl_surface = wl_egl_surface;

        self.make_current()
            .wrap_err("Failed to switch to the EGL context")?;
        self.renderer
            .resize(display_info)
            .wrap_err("Failed to resize GL window")?;
        // If we resize, stop immediately any lingering transition
        self.renderer.transition_finished();

        Ok(())
    }

    pub fn load_wallpaper(
        &mut self,
        image: DynamicImage,
        background_mode: BackgroundMode,
        offset: Option<f32>,
        display_info: &DisplayInfo,
    ) -> Result<()> {
        // Renderer::load_wallpaper load the wallpaper in a openGL texture
        // Set the correct opengl context
        self.make_current()
            .wrap_err("Failed to switch EGL context")?;
        self.renderer
            .load_wallpaper(image, background_mode, offset, display_info)
    }

    pub fn draw(&mut self) -> Result<()> {
        unsafe { self.renderer.draw()? }

        self.renderer
            .clear_after_draw()
            .wrap_err("Failed to unbind the buffer")?;
        self.swap_buffers().wrap_err("Failed to swap EGL buffers")?;

        // Reset the context
        egl::API
            .make_current(self.display, None, None, None)
            .wrap_err("Failed to reset the EGL context")
    }
}

impl Drop for EglContext {
    fn drop(&mut self) {
        if let Err(err) = egl.destroy_surface(self.display, self.surface) {
            warn!(
                "{:?}",
                eyre!(err).wrap_err(format!(
                    "Failed to destroy surface for display {}",
                    self.display_name
                ))
            );
        }
    }
}
