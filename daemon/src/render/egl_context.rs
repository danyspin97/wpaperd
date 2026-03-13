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
    ///
    /// Rather than destroying and recreating the EGL surface, we resize the
    /// existing `wl_egl_window` in place via `wl_egl_window_resize()`. This
    /// avoids calling `eglDestroyS()` on the currently-bound surface, which
    /// on NVIDIA driver 575.64+ triggers an implicit:
    ///
    /// eglMakeCurrent(NO_SURFACE, NO_SURFACE, NO_CONTEXT)
    ///
    /// That implicit release corrupts the driver's "current draw surface"
    /// tracking, causing the next `eglSwapBuffers()` to fail with
    /// EGL_BAD_SURFACE.
    ///
    /// See:
    /// - https://forums.developer.nvidia.com/t/575-64-libegl-nvidia-so-bug-eglsurface-x-is-not-current-draw-surface/336704
    /// - wpaperd issues #116, #129, and #149
    pub fn resize(&mut self, display_info: &DisplayInfo) -> Result<()> {
        self.wl_egl_surface.resize(
            display_info.adjusted_width(),
            display_info.adjusted_height(),
            0,
            0,
        );
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

        // We intentionally do not release the EGL context after swapping
        // buffers. libEGL_nvidia.so in NVIDIA driver 575.64+ appears to have
        // a bug which causes:
        //
        // eglMakeCurrent(dpy, NO_SURFACE, NO_SURFACE, NO_CONTEXT)
        //
        // Followed by a subsequent:
        //
        // eglMakeCurrent(dpy, surf, surf, ctx)
        //
        // To leaves the driver in a confused state, where the next call to
        // `eglSwapBuffers()` will fail with:
        //
        // EGL_BAD_SURFACE ("EGLSurface is not current draw surface")
        //
        // Per EGL spec §3.7.3 the context is implicitly released when another
        // surface calls make_current(), so explicit release is not necessary
        // on a single-threaded renderer.
        //
        // See https://registry.khronos.org/EGL/specs/eglspec.1.5.pdf
        self.swap_buffers().wrap_err("Failed to swap EGL buffers")
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
