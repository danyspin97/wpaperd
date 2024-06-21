use std::cell::RefCell;
use std::rc::Rc;

use color_eyre::owo_colors::OwoColorize;
use color_eyre::Result;
use log::{error, warn};
use smithay_client_toolkit::compositor::{CompositorHandler, CompositorState, Region};
use smithay_client_toolkit::output::{OutputHandler, OutputState};
use smithay_client_toolkit::reexports::calloop::LoopHandle;
use smithay_client_toolkit::reexports::client::globals::GlobalList;
use smithay_client_toolkit::reexports::client::protocol::{wl_output, wl_surface};
use smithay_client_toolkit::reexports::client::{Connection, QueueHandle};
use smithay_client_toolkit::registry::{ProvidesRegistryState, RegistryState};
use smithay_client_toolkit::shell::wlr_layer::{
    Anchor, Layer, LayerShell, LayerShellHandler, LayerSurface, LayerSurfaceConfigure,
};
use smithay_client_toolkit::shm::{Shm, ShmHandler};
use smithay_client_toolkit::{
    delegate_compositor, delegate_layer, delegate_output, delegate_registry, delegate_shm,
    registry_handlers,
};

use crate::config::Config;
use crate::display_info::DisplayInfo;
use crate::filelist_cache::FilelistCache;
use crate::image_loader::ImageLoader;
use crate::surface::Surface;
use crate::wallpaper_info::WallpaperInfo;

pub struct Wpaperd {
    pub compositor_state: CompositorState,
    pub output_state: OutputState,
    pub shm_state: Shm,
    pub layer_state: LayerShell,
    pub registry_state: RegistryState,
    pub surfaces: Vec<Surface>,
    pub config: Config,
    egl_display: egl::Display,
    pub filelist_cache: Rc<RefCell<FilelistCache>>,
    pub image_loader: Rc<RefCell<ImageLoader>>,
}

impl Wpaperd {
    pub fn new(
        qh: &QueueHandle<Self>,
        globals: &GlobalList,
        config: Config,
        egl_display: egl::Display,
        filelist_cache: Rc<RefCell<FilelistCache>>,
    ) -> Result<Self> {
        let shm_state = Shm::bind(globals, qh)?;

        let image_loader = Rc::new(RefCell::new(ImageLoader::new()));

        Ok(Self {
            compositor_state: CompositorState::bind(globals, qh)?,
            output_state: OutputState::new(globals, qh),
            shm_state,
            layer_state: LayerShell::bind(globals, qh)?,
            registry_state: RegistryState::new(globals),
            surfaces: Vec::new(),
            config,
            egl_display,
            filelist_cache,
            image_loader,
        })
    }

    pub fn update_surfaces(&mut self, ev_handle: LoopHandle<Wpaperd>, qh: &QueueHandle<Wpaperd>) {
        for surface in &mut self.surfaces {
            let res = self.config.get_output_by_name(&surface.name());
            match res {
                Ok(wallpaper_info) => {
                    surface.update_wallpaper_info(&ev_handle, qh, wallpaper_info);
                }
                Err(err) => warn!(
                    "Configuration error for display {}: {err:?}",
                    surface.name()
                ),
            }
        }
    }

    pub fn surface_from_name(&mut self, name: &str) -> Option<&mut Surface> {
        self.surfaces
            .iter_mut()
            .find(|surface| surface.name() == name)
    }

    pub fn surface_from_wl_surface(&mut self, surface: &wl_surface::WlSurface) -> &mut Surface {
        self.surfaces
            .iter_mut()
            .find(|s| surface == s.wl_surface())
            .expect("surface to be registered in wpaperd")
    }
}

impl CompositorHandler for Wpaperd {
    fn scale_factor_changed(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        surface: &wl_surface::WlSurface,
        new_factor: i32,
    ) {
        self.surface_from_wl_surface(surface)
            .change_scale_factor(new_factor, qh);
    }

    fn frame(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        surface: &wl_surface::WlSurface,
        time: u32,
    ) {
        let surface = self.surface_from_wl_surface(surface);

        match surface.draw(qh, Some(time)) {
            Ok(_) => {}
            Err(err) => {
                error!("Error drawing surface: {err:?}");
            }
        }
    }

    fn transform_changed(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        surface: &wl_surface::WlSurface,
        new_transform: wl_output::Transform,
    ) {
        self.surface_from_wl_surface(surface)
            .change_transform(new_transform, qh);
    }
}

impl OutputHandler for Wpaperd {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        output: wl_output::WlOutput,
    ) {
        let surface = self.compositor_state.create_surface(qh);

        let info = match self.output_state.info(&output) {
            Some(info) => info,
            None => {
                error!("could not get info about output");
                return;
            }
        };
        surface.set_buffer_scale(info.scale_factor);
        surface.set_buffer_transform(info.transform);

        let name = info
            .name
            .as_ref()
            .map(|name| name.to_string())
            .unwrap_or_else(|| "unnamed".to_string());
        let display_info = DisplayInfo::new(info);

        let layer = self.layer_state.create_layer_surface(
            qh,
            surface.clone(),
            Layer::Background,
            Some(format!("wpaperd-{}", name)),
            Some(&output),
        );
        layer.set_anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT | Anchor::BOTTOM);
        layer.set_exclusive_zone(-1);
        layer.set_size(
            display_info.adjusted_width() as u32,
            display_info.adjusted_height() as u32,
        );

        match Region::new(&self.compositor_state) {
            Ok(region) => {
                // Wayland clients are expected to render the cursor on their input region. By setting the
                // input region to an empty region, the compositor renders the default cursor. Without
                // this, and empty desktop won't render a cursor.
                surface.set_input_region(Some(region.wl_region()));

                // From `wl_surface::set_opaque_region`:
                // > Setting the pending opaque region has copy semantics, and the
                // > wl_region object can be destroyed immediately.
                region.wl_region().destroy();
            }

            Err(_) => {
                warn!("could not create region, cursor won't be shown for display {name}");
                return;
            }
        };

        let wallpaper_info = match self.config.get_output_by_name(&name) {
            Ok(wallpaper_info) => wallpaper_info,
            Err(err) => {
                warn!(
                    "Configuration error on display {}: {err:?}",
                    name.bold().magenta()
                );
                WallpaperInfo::default()
            }
        };

        self.surfaces.push(Surface::new(
            layer,
            output,
            display_info,
            wallpaper_info,
            self.egl_display,
            self.filelist_cache.clone(),
            self.image_loader.clone(),
        ));
    }

    fn update_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
        // TODO: Do we need to do something here?
    }

    fn output_destroyed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        output: wl_output::WlOutput,
    ) {
        // Find the destroyed output and remove it

        match self
            .surfaces
            .iter()
            .enumerate()
            .find(|(_, surface)| *surface.wl_output() == output)
        {
            Some((index, _)) => {
                self.surfaces.swap_remove(index);
            }
            None => error!("could not find display while handling output_destroyed"),
        }
    }
}

impl LayerShellHandler for Wpaperd {
    fn closed(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _layer: &LayerSurface) {}

    fn configure(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        layer: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _serial: u32,
    ) {
        match self
            .surfaces
            .iter_mut()
            .find(|surface| surface.layer() == layer)
        {
            Some(surface) => surface.change_size(configure, qh),
            None => error!("could not find display while handling configure in wayland"),
        }
    }
}

impl ShmHandler for Wpaperd {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm_state
    }
}

delegate_compositor!(Wpaperd);
delegate_output!(Wpaperd);
delegate_shm!(Wpaperd);
delegate_registry!(Wpaperd);
delegate_layer!(Wpaperd);

impl ProvidesRegistryState for Wpaperd {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState];
}
