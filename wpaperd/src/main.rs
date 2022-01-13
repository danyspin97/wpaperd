mod config;
mod output;
mod output_timer;
mod surface;

use std::{
    cell::RefCell,
    collections::HashMap,
    path::PathBuf,
    process::exit,
    rc::Rc,
    sync::{Arc, Mutex},
};

use calloop::channel::Sender;
use clap::Parser;
use color_eyre::{eyre::WrapErr, Result};
use hotwatch::{Event, Hotwatch};
use log::error;
use nix::unistd::fork;
use output_timer::OutputTimer;
use simplelog::{ColorChoice, LevelFilter, TermLogger, TerminalMode};
use smithay_client_toolkit::{
    environment,
    environment::SimpleGlobal,
    output::{
        with_output_info, OutputHandler, OutputHandling, OutputInfo, OutputStatusListener,
        XdgOutputHandler,
    },
    reexports::{
        calloop,
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

#[derive(Parser)]
#[clap(
    author = "Danilo Spinella <danilo.spinella@suse.com>",
    version,
    about = "A wallpaper manager for Wayland compositors"
)]
struct Opts {
    #[clap(short, long, help = "Path to the config to read")]
    config: Option<PathBuf>,
    #[clap(
        short = 'n',
        long = "no-daemon",
        help = "Stay in foreground, do not detach"
    )]
    no_daemon: bool,
}

fn get_timer_closure(surface_timer: Arc<Mutex<OutputTimer>>, tx: Sender<()>) -> impl Fn() {
    move || {
        if surface_timer.lock().unwrap().check_timeout() {
            tx.send(()).unwrap();
        }
    }
}

fn main() -> Result<()> {
    color_eyre::install()?;
    TermLogger::init(
        LevelFilter::Warn,
        simplelog::Config::default(),
        TerminalMode::Mixed,
        ColorChoice::Auto,
    )?;

    let opts = Opts::parse();

    if !opts.no_daemon {
        match unsafe { fork()? } {
            nix::unistd::ForkResult::Parent { child: _ } => exit(0),
            nix::unistd::ForkResult::Child => {}
        }
    }

    let config_file = if let Some(config_file) = opts.config {
        config_file
    } else {
        let xdg_dirs = BaseDirectories::with_prefix("wpaper").unwrap();
        xdg_dirs.place_config_file("wpaperd.conf").unwrap()
    };
    let mut config = Config::new_from_path(&config_file)?;
    config.reloaded = false;
    let config = Arc::new(Mutex::new(config));
    let display = Display::connect_to_env().unwrap();
    let mut queue = display.create_event_queue();
    let (outputs, xdg_output) =
        smithay_client_toolkit::output::XdgOutputHandler::new_output_handlers();

    struct Status {
        env: environment::Environment<Env>,
        surfaces: RefCell<Vec<(u32, Surface)>>,
    }

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

    let layer_shell = env.require_global::<zwlr_layer_shell_v1::ZwlrLayerShellV1>();

    let config_clone = config.clone();
    let status_rc = status.clone();
    let output_handler = move |output: wl_output::WlOutput, info: &OutputInfo| {
        if info.obsolete {
            // an output has been removed, release it
            status_rc
                .surfaces
                .borrow_mut()
                .retain(|(i, _)| *i != info.id);
            output.release();
        } else {
            // an output has been created, construct a surface for it
            let surface = status_rc.env.create_surface().detach();
            let pool = status_rc
                .env
                .create_auto_pool()
                .expect("failed to create a memory pool!");
            let config = config_clone.lock().unwrap();
            (*status_rc.surfaces.borrow_mut()).push((
                info.id,
                Surface::new(
                    &output,
                    surface,
                    &layer_shell.clone(),
                    info.clone(),
                    pool,
                    config.get_output_by_name(&info.name),
                ),
            ));
        }
    };

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

    let ev_tx_clone = ev_tx.clone();
    let config_clone = config.clone();
    let mut hotwatch = Hotwatch::new().context("hotwatch failed to initialize")?;
    hotwatch
        .watch(&config_file, move |event: Event| {
            if let Event::Write(_) = event {
                let mut config = config_clone.lock().unwrap();
                let new_config = Config::new_from_path(&config.path)
                    .with_context(|| format!("reading configuration from file {:?}", config.path));
                match new_config {
                    Ok(new_config) => {
                        *config = new_config;
                        ev_tx_clone.send(()).unwrap();
                    }
                    Err(err) => {
                        error!("{:?}", err);
                    }
                }
            }
        })
        .with_context(|| format!("watching file {:?}", &config_file))?;

    let timer = timer::Timer::new();

    let mut timer_guards = HashMap::new();
    macro_rules! add_timer_on_draw {
        ($surface:ident) => {
            if $surface
                .draw()
                .with_context(|| format!("drawing surface for {}", $surface.info.name))?
            {
                if let Some(duration) = $surface.output.duration {
                    timer_guards.insert(
                        $surface.info.id,
                        timer.schedule_with_delay(
                            chrono::Duration::seconds(duration.as_secs().try_into().unwrap()),
                            get_timer_closure($surface.timer.clone(), ev_tx.clone()),
                        ),
                    );
                }
            }
        };
    }
    loop {
        let mut surfaces = status.surfaces.borrow_mut();
        {
            let mut config = config.lock().unwrap();
            if config.reloaded {
                for (_, surface) in surfaces.iter_mut() {
                    surface.update_output(config.get_output_by_name(&surface.info.name));
                    add_timer_on_draw!(surface);
                }
                config.reloaded = false;
            }
        }

        // This is ugly, let's hope that some version of drain_filter() gets stabilized soon
        // https://github.com/rust-lang/rust/issues/43244
        let mut removal = Vec::new();
        {
            let mut i = 0;
            while i != surfaces.len() {
                let surface = &mut surfaces.get_mut(i).unwrap().1;
                if surface.handle_events() {
                    removal.push(i);
                } else {
                    add_timer_on_draw!(surface);
                }
                i += 1;
            }
        }
        for i in removal {
            timer_guards.remove(&surfaces.get(i).unwrap().1.info.id);
            surfaces.remove(i);
        }

        display.flush().context("flushing the display")?;
        event_loop
            .dispatch(None, &mut ())
            .context("dispatching the event loop")?;
    }
}
