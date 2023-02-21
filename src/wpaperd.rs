use std::sync::{Arc, Mutex};

use color_eyre::Result;
use smithay_client_toolkit::compositor::{CompositorHandler, CompositorState};
use smithay_client_toolkit::output::{OutputHandler, OutputState};
use smithay_client_toolkit::reexports::client::globals::{GlobalList, GlobalListContents};
use smithay_client_toolkit::reexports::client::protocol::wl_registry;
use smithay_client_toolkit::reexports::client::protocol::{wl_output, wl_surface};
use smithay_client_toolkit::reexports::client::{Connection, Dispatch, QueueHandle};
use smithay_client_toolkit::registry::{ProvidesRegistryState, RegistryState};
use smithay_client_toolkit::shell::layer::{
    LayerShell, LayerShellHandler, LayerSurface, LayerSurfaceConfigure,
};
use smithay_client_toolkit::shm::{ShmHandler, ShmState};
use smithay_client_toolkit::{
    delegate_compositor, delegate_layer, delegate_output, delegate_shm, registry_handlers,
};

use crate::surface::Surface;
use crate::wallpaper_config::WallpaperConfig;

pub struct Wpaperd {
    pub compositor_state: CompositorState,
    pub output_state: OutputState,
    pub shm_state: ShmState,
    pub layer_state: LayerShell,
    pub registry_state: RegistryState,
    pub surfaces: Vec<Surface>,
    wallpaper_config: Arc<Mutex<WallpaperConfig>>,
    use_scaled_window: bool,
}

impl Wpaperd {
    pub fn new(
        qh: &QueueHandle<Self>,
        globals: &GlobalList,
        _conn: &Connection,
        wallpaper_config: Arc<Mutex<WallpaperConfig>>,
        use_scaled_window: bool,
    ) -> Result<Self> {
        let shm_state = ShmState::bind(globals, qh)?;
        Ok(Self {
            compositor_state: CompositorState::bind(globals, qh)?,
            output_state: OutputState::new(globals, qh),
            shm_state,
            layer_state: LayerShell::bind(globals, qh)?,
            registry_state: RegistryState::new(globals),
            surfaces: Vec::new(),
            wallpaper_config,
            use_scaled_window,
        })
    }
}

impl CompositorHandler for Wpaperd {
    fn scale_factor_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        surface: &wl_surface::WlSurface,
        new_factor: i32,
    ) {
        let mut surface = self
            .surfaces
            .iter_mut()
            .enumerate()
            .find(|(_, s)| surface == &s.surface)
            .unwrap()
            .1;

        // Ignore unnecessary updates
        if surface.scale != new_factor {
            surface.scale = new_factor;
            surface.surface.set_buffer_scale(new_factor);
            surface.need_redraw = true;
        }
    }

    fn frame(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _time: u32,
    ) {
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
        // TODO: Error handling
        let surface = self.compositor_state.create_surface(qh);

        let info = self.output_state.info(&output).unwrap();
        let scale = if self.use_scaled_window {
            1
        } else {
            info.scale_factor
        };
        surface.set_buffer_scale(scale);

        let name = info.name.as_ref().unwrap().to_string();

        self.surfaces.push(Surface::new(
            qh,
            output,
            &self.layer_state,
            surface,
            &self.shm_state,
            info,
            self.wallpaper_config
                .lock()
                .unwrap()
                .get_output_by_name(&name),
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
        self.surfaces.swap_remove(
            self.surfaces
                .iter()
                .enumerate()
                .find(|(_, surface)| surface.output == output)
                .unwrap()
                .0,
        );
    }
}

impl LayerShellHandler for Wpaperd {
    fn closed(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _layer: &LayerSurface) {}

    fn configure(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        layer: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _serial: u32,
    ) {
        let surface = self
            .surfaces
            .iter_mut()
            .find(|surface| &surface.layer == layer)
            // We always know the surface that it is being configured
            .unwrap();

        if surface.dimensions != configure.new_size {
            // Update dimensions
            surface.dimensions = configure.new_size;
            surface.need_redraw = true;
        }

        surface.configured = true;
    }
}

impl ShmHandler for Wpaperd {
    fn shm_state(&mut self) -> &mut ShmState {
        &mut self.shm_state
    }
}

delegate_compositor!(Wpaperd);
delegate_output!(Wpaperd);
delegate_shm!(Wpaperd);

delegate_layer!(Wpaperd);

impl ProvidesRegistryState for Wpaperd {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState];
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for Wpaperd {
    fn event(
        _state: &mut Self,
        _registry: &wl_registry::WlRegistry,
        _event: wl_registry::Event,
        _data: &GlobalListContents,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        // We don't need any other globals.
    }
}
