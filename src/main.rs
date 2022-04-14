mod config;
mod output;
mod output_config;
mod surface;

use std::{
    cell::RefCell,
    fs,
    path::Path,
    process::exit,
    rc::Rc,
    sync::{Arc, Mutex},
    time::Instant,
};

use clap::StructOpt;
use color_eyre::{eyre::WrapErr, Result};
use flexi_logger::{Duplicate, FileSpec, Logger};
use hotwatch::{Event, Hotwatch};
use log::{error, trace};
use nix::unistd::fork;
use smithay_client_toolkit::{
    environment,
    environment::SimpleGlobal,
    output::{
        with_output_info, OutputHandler, OutputHandling, OutputInfo, OutputStatusListener,
        XdgOutputHandler,
    },
    reexports::{
        calloop::{self, channel::Sender},
        client::{
            protocol::{
                wl_compositor::WlCompositor,
                wl_output::{self, WlOutput},
                wl_shm::WlShm,
            },
            DispatchData, Display,
        },
        protocols::{
            unstable::xdg_output::v1::client::zxdg_output_manager_v1,
            wlr::unstable::layer_shell::v1::client::zwlr_layer_shell_v1,
        },
    },
    shm::ShmHandler,
    WaylandSource,
};
use xdg::BaseDirectories;

use crate::config::Config;
use crate::output_config::OutputConfig;
use crate::surface::Surface;

struct Env {
    compositor: SimpleGlobal<WlCompositor>,
    outputs: OutputHandler,
    shm: ShmHandler,
    xdg_output: XdgOutputHandler,
    layer_shell: SimpleGlobal<zwlr_layer_shell_v1::ZwlrLayerShellV1>,
}

environment!(Env,
    singles = [
        WlCompositor => compositor,
        WlShm => shm,
        zwlr_layer_shell_v1::ZwlrLayerShellV1 => layer_shell,
        zxdg_output_manager_v1::ZxdgOutputManagerV1 => xdg_output,
    ],
    multis = [
        WlOutput => outputs,
    ]
);

impl OutputHandling for Env {
    fn listen<F>(&mut self, f: F) -> OutputStatusListener
    where
        F: FnMut(WlOutput, &OutputInfo, DispatchData) + 'static,
    {
        self.outputs.listen(f)
    }
}

struct Status {
    env: environment::Environment<Env>,
    surfaces: RefCell<Vec<(u32, Surface, Option<timer::Guard>)>>,
}

fn main() -> Result<()> {
    color_eyre::install()?;

    let xdg_dirs = BaseDirectories::with_prefix("wpaperd")?;

    let opts = Config::parse();
    let config_file = if let Some(config_file) = &opts.config {
        config_file.clone()
    } else {
        xdg_dirs.place_config_file("wpaperd.conf").unwrap()
    };

    let mut config: Config = if config_file.exists() {
        toml::from_str(&fs::read_to_string(config_file)?)?
    } else {
        Config::default()
    };
    config.merge(opts);

    let mut logger = Logger::try_with_env_or_str("info")?;

    if config.no_daemon {
        logger = logger.duplicate_to_stderr(Duplicate::Warn);
    } else {
        logger = logger.log_to_file(FileSpec::default().directory(xdg_dirs.get_state_home()));
        match unsafe { fork()? } {
            nix::unistd::ForkResult::Parent { child: _ } => exit(0),
            nix::unistd::ForkResult::Child => {}
        }
    }
    logger.start()?;

    let output_config_file = if let Some(output_config_file) = config.output_config {
        output_config_file
    } else {
        xdg_dirs.place_config_file("output.conf").unwrap()
    };
    let mut output_config = OutputConfig::new_from_path(&output_config_file)?;
    output_config.reloaded = false;
    let output_config = Arc::new(Mutex::new(output_config));
    let display = Display::connect_to_env().unwrap();
    let mut queue = display.create_event_queue();
    let (outputs, xdg_output) =
        smithay_client_toolkit::output::XdgOutputHandler::new_output_handlers();

    let status = Rc::new(Status {
        env: smithay_client_toolkit::environment::Environment::new(
            &display.attach(queue.token()),
            &mut queue,
            Env {
                compositor: SimpleGlobal::new(),
                outputs,
                shm: ShmHandler::new(),
                xdg_output,
                layer_shell: SimpleGlobal::new(),
            },
        )
        .unwrap(),
        surfaces: RefCell::new(Vec::new()),
    });

    let env = &status.env;
    let mut output_handler = output_handler(status.clone(), output_config.clone());
    // Process currently existing outputs
    for output in env.get_all_outputs() {
        if let Some(info) = with_output_info(&output, Clone::clone) {
            output_handler(output, &info);
        }
    }

    // Setup a listener for changes
    // The listener will live for as long as we keep this handle alive
    let _listner_handle =
        env.listen_for_outputs(move |output, info, _| output_handler(output, info));

    let mut event_loop = calloop::EventLoop::<()>::try_new()?;

    WaylandSource::new(queue)
        .quick_insert(event_loop.handle())
        .unwrap();

    let (ev_tx, ev_rx) = calloop::channel::channel();
    event_loop
        .handle()
        .insert_source(ev_rx, |_, _, _| {})
        .unwrap();

    let _hotwatch = setup_hotwatch(&output_config_file, output_config.clone(), ev_tx.clone());

    let timer = timer::Timer::new();

    loop {
        {
            let mut output_config = output_config.lock().unwrap();
            if output_config.reloaded {
                let mut surfaces = status.surfaces.borrow_mut();
                for (_, surface, _) in surfaces.iter_mut() {
                    surface.update_output(output_config.get_output_by_name(&surface.info.name));
                }
                output_config.reloaded = false;
            }
        }

        let surfaces = status.surfaces.take();
        let process_surface_event = process_surface_event(&timer, ev_tx.clone());
        status.surfaces.replace(
            surfaces
                .into_iter()
                .filter_map(process_surface_event)
                .collect::<Vec<(u32, Surface, Option<timer::Guard>)>>(),
        );

        display.flush().context("flushing the display")?;
        event_loop
            .dispatch(None, &mut ())
            .context("dispatching the event loop")?;
    }
}

fn output_handler(
    status: Rc<Status>,
    output_config: Arc<Mutex<OutputConfig>>,
) -> Box<dyn FnMut(WlOutput, &OutputInfo)> {
    let layer_shell = status
        .env
        .require_global::<zwlr_layer_shell_v1::ZwlrLayerShellV1>();

    Box::new(move |output: wl_output::WlOutput, info: &OutputInfo| {
        if info.obsolete {
            // an output has been removed, release it
            status
                .surfaces
                .borrow_mut()
                .retain(|(i, _, _)| *i != info.id);
            output.release();
        } else {
            // an output has been created, construct a surface for it
            let surface = status.env.create_surface().detach();
            let pool = status
                .env
                .create_auto_pool()
                .expect("failed to create a memory pool!");
            let output_config = output_config.lock().unwrap();
            (*status.surfaces.borrow_mut()).push((
                info.id,
                Surface::new(
                    &output,
                    surface,
                    &layer_shell.clone(),
                    info.clone(),
                    pool,
                    output_config.get_output_by_name(&info.name),
                ),
                None,
            ));
        }
    })
}

fn setup_hotwatch(
    output_config_file: &Path,
    output_config: Arc<Mutex<OutputConfig>>,
    ev_tx: Sender<()>,
) -> Result<Hotwatch> {
    let mut hotwatch = Hotwatch::new().context("hotwatch failed to initialize")?;
    hotwatch
        .watch(&output_config_file, move |event: Event| {
            if let Event::Write(_) = event {
                let mut output_config = output_config.lock().unwrap();
                let new_config =
                    OutputConfig::new_from_path(&output_config.path).with_context(|| {
                        format!("reading configuration from file {:?}", output_config.path)
                    });
                match new_config {
                    Ok(new_config) => {
                        *output_config = new_config;
                        ev_tx.send(()).unwrap();
                    }
                    Err(err) => {
                        error!("{:?}", err);
                    }
                }
            }
        })
        .with_context(|| format!("watching file {output_config_file:?}"))?;
    Ok(hotwatch)
}

type SurfaceEvent = (u32, Surface, Option<timer::Guard>);

fn process_surface_event<'a>(
    timer: &'a timer::Timer,
    ev_tx: Sender<()>,
) -> Box<dyn FnMut(SurfaceEvent) -> Option<SurfaceEvent> + 'a> {
    Box::new(move |(id, mut surface, guard): (u32, Surface, Option<timer::Guard>)|
      -> Option<(u32, Surface, Option<timer::Guard>)> {
    if surface.handle_events() {
        None
    } else {
        let now = Instant::now();
        trace!("iterating over output {}", surface.info.name);
        let guard = if surface.should_draw(&now) {
            trace!("drawing output {}", surface.info.name);
            surface
                .draw(now)
                .with_context(|| format!("drawing surface for {}", surface.info.name))
                .unwrap();
            if let Some(duration) = surface.output.duration {
                let ev_tx = ev_tx.clone();
                Some(timer.schedule_with_delay(duration, move || {
                    ev_tx.send(()).unwrap();
                }))
            } else {
                None
            }
        } else {
            guard
        };
        Some((id, surface, guard))
    }
    })
}
