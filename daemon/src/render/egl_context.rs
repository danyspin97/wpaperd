use smithay_client_toolkit::reexports::client::{protocol::wl_surface::WlSurface, Proxy};
use wayland_egl::WlEglSurface;

use egl::API as egl;

use color_eyre::{eyre::Context, Result};

pub struct EglContext {
    pub display: egl::Display,
    pub context: egl::Context,
    pub config: egl::Config,
    wl_egl_surface: WlEglSurface,
    surface: khronos_egl::Surface,
}

impl EglContext {
    pub fn new(egl_display: egl::Display, wl_surface: &WlSurface) -> Self {
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
            .expect("unable to choose an EGL configuration")
            .expect("no EGL configuration found");

        const CONTEXT_ATTRIBUTES: [i32; 5] = [
            egl::CONTEXT_MAJOR_VERSION,
            2,
            egl::CONTEXT_MINOR_VERSION,
            0,
            egl::NONE,
        ];

        let context = egl
            .create_context(egl_display, config, None, &CONTEXT_ATTRIBUTES)
            .expect("unable to create an EGL context");

        // First, create a small surface, we don't know the size of the output yet
        let wl_egl_surface = WlEglSurface::new(wl_surface.id(), 10, 10).unwrap();

        let surface = unsafe {
            egl.create_window_surface(
                egl_display,
                config,
                wl_egl_surface.ptr() as egl::NativeWindowType,
                None,
            )
            .expect("unable to create an EGL surface")
        };

        Self {
            display: egl_display,
            context,
            config,
            surface,
            wl_egl_surface,
        }
    }

    #[inline]
    pub fn make_current(&self) -> Result<()> {
        egl.make_current(
            self.display,
            Some(self.surface),
            Some(self.surface),
            Some(self.context),
        )
        .with_context(|| "unable to make the context current")
    }

    // Swap the buffers of the surface
    #[inline]
    pub fn swap_buffers(&self) -> Result<()> {
        egl.swap_buffers(self.display, self.surface)
            .with_context(|| "unable to post the surface content")
    }

    /// Resize the surface
    /// Resizing the surface means to destroy the previous one and then recreate it
    pub fn resize(&mut self, wl_surface: &WlSurface, width: i32, height: i32) -> Result<()> {
        egl.destroy_surface(self.display, self.surface)
            .context("unable to destroy EGL surface")?;
        let wl_egl_surface = WlEglSurface::new(wl_surface.id(), width, height)
            .context("unable to create a new WlEglSurface")?;

        let surface = unsafe {
            egl.create_window_surface(
                self.display,
                self.config,
                wl_egl_surface.ptr() as egl::NativeWindowType,
                None,
            )
            .context("unable to create an EGL surface")?
        };

        self.surface = surface;
        self.wl_egl_surface = wl_egl_surface;

        Ok(())
    }
}
