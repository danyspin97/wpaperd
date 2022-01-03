mod config;
mod output;
mod output_timer;
mod surface;

use std::{
    cell::RefCell,
    collections::HashMap,
    rc::Rc,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};

use calloop::channel::Sender;
use color_eyre::{eyre::ensure, Result};
use hotwatch::{Event, Hotwatch};
use output_timer::OutputTimer;
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

fn get_timer_closure(surface_timer: Arc<Mutex<OutputTimer>>, tx: Sender<()>) -> impl Fn() {
    move || {
        if surface_timer.lock().unwrap().check_timeout() {
            tx.send(()).unwrap();
        }
    }
}

fn main() -> Result<()> {
    color_eyre::install()?;

    let xdg_dirs = BaseDirectories::with_prefix("wpaper").unwrap();
    let config_file = xdg_dirs.place_config_file("wpaperd.conf").unwrap();
    ensure!(
        config_file.exists(),
        "Configuration file {:?} does not exists",
        config_file
    );
    let config = Arc::new(Mutex::new(Config::new_from_path(&config_file).unwrap()));
    let display = Display::connect_to_env().unwrap();
    let mut queue = display.create_event_queue();
    let (outputs, xdg_output) =
        smithay_client_toolkit::output::XdgOutputHandler::new_output_handlers();

    let env = smithay_client_toolkit::environment::Environment::new(
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
    .unwrap();

    let surfaces = Rc::new(RefCell::new(Vec::new()));

    let layer_shell = env.require_global::<zwlr_layer_shell_v1::ZwlrLayerShellV1>();

    let env_handle = env.clone();
    let surfaces_handle = Rc::clone(&surfaces);
    let config_clone = config.clone();
    let output_handler = move |output: wl_output::WlOutput, info: &OutputInfo| {
        if info.obsolete {
            // an output has been removed, release it
            surfaces_handle.borrow_mut().retain(|(i, _)| *i != info.id);
            output.release();
        } else {
            // an output has been created, construct a surface for it
            let surface = env_handle.create_surface().detach();
            let pool = env_handle
                .create_auto_pool()
                .expect("Failed to create a memory pool!");
            (*surfaces_handle.borrow_mut()).push((
                info.id,
                Surface::new(
                    &output,
                    surface,
                    &layer_shell.clone(),
                    info.clone(),
                    pool,
                    config_clone.lock().unwrap().get_output_by_name(&info.name),
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

    let mut event_loop = calloop::EventLoop::<()>::try_new().unwrap();

    WaylandSource::new(queue)
        .quick_insert(event_loop.handle())
        .unwrap();

    let (ev_tx, ev_rx) = calloop::channel::channel();
    event_loop
        .handle()
        .insert_source(ev_rx, |_, _, _| {})
        .unwrap();

    let ev_tx_clone = ev_tx.clone();
    let config_reloaded = Arc::new(AtomicBool::new(false));
    let config_reloaded_clone = Arc::clone(&config_reloaded);
    let config_clone = config.clone();
    let config_file_clone = config_file.clone();
    let mut hotwatch = Hotwatch::new().expect("Hotwatch failed to initialize.");
    hotwatch
        .watch(&config_file, move |event: Event| {
            if let Event::Write(_) = event {
                let new_config = Config::new_from_path(&config_file_clone);
                if let Ok(new_config) = new_config {
                    *config_clone.lock().unwrap() = new_config;
                    config_reloaded_clone.store(true, Ordering::SeqCst);
                    ev_tx_clone.send(()).unwrap();
                }
            }
        })
        .expect("Failed to watch file!");

    let timer = timer::Timer::new();

    let mut timer_guards = HashMap::new();
    macro_rules! add_timer_on_draw {
        ($surface:ident) => {
            if let Some(duration) = $surface.draw().unwrap() {
                timer_guards.insert(
                    $surface.info.id,
                    timer.schedule_with_delay(
                        chrono::Duration::seconds(duration.into()),
                        get_timer_closure($surface.timer.clone(), ev_tx.clone()),
                    ),
                );
            }
        };
    }
    loop {
        if config_reloaded.load(Ordering::SeqCst) {
            let config = config.lock().unwrap();
            let mut surfaces = surfaces.borrow_mut();
            for (_, surface) in surfaces.iter_mut() {
                surface.update_output(config.get_output_by_name(&surface.info.name));
                add_timer_on_draw!(surface);
            }
            config_reloaded.store(false, Ordering::SeqCst);
        } else {
            // This is ugly, let's hope that some version of drain_filter() gets stabilized soon
            // https://github.com/rust-lang/rust/issues/43244
            let mut removal = Vec::new();
            {
                let mut surfaces = surfaces.borrow_mut();
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
            let mut surfaces = surfaces.borrow_mut();
            for i in removal {
                timer_guards.remove(&surfaces.get(i).unwrap().1.info.id);
                surfaces.remove(i);
            }
        }

        display.flush().unwrap();
        event_loop.dispatch(None, &mut ()).unwrap();
    }
}
