mod config;
mod display_info;
mod filelist_cache;
mod image_loader;
mod image_picker;
mod ipc_server;
mod opts;
mod render;
mod socket;
mod surface;
mod wallpaper_groups;
mod wallpaper_info;
mod wpaperd;

extern crate khronos_egl as egl;

use std::{
    cell::RefCell,
    fs::File,
    io::Write,
    os::fd::FromRawFd,
    process::exit,
    rc::Rc,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use clap::Parser;
use color_eyre::{
    eyre::{anyhow, ContextCompat, WrapErr},
    Result, Section,
};
use config::Config;
use egl::API as egl;
use filelist_cache::FilelistCache;
use flexi_logger::{Duplicate, FileSpec, Logger};
use hotwatch::Hotwatch;
use ipc_server::{handle_message, listen_on_ipc_socket};
use log::error;
use nix::unistd::fork;
use opts::Opts;
use smithay_client_toolkit::reexports::{
    calloop,
    calloop_wayland_source::WaylandSource,
    client::{globals::registry_queue_init, Connection, Proxy},
};
use wallpaper_groups::WallpaperGroups;
use wallpaper_info::Sorting;
use wpaperd_ipc::socket_path;
use xdg::BaseDirectories;

use crate::wpaperd::Wpaperd;

use tikv_jemallocator::Jemalloc;

#[global_allocator]
static GLOBAL: Jemalloc = Jemalloc;

fn run(opts: Opts, xdg_dirs: BaseDirectories) -> Result<()> {
    // Path passed from the CLI or the wpaperd.toml file has precedence
    let config_file = if let Some(config) = opts.config {
        config
    } else {
        // Read the new config or the legacy file
        let legacy_config_file = xdg_dirs
            .place_config_file("wallpaper.toml")
            .context("unable to identify legacy config file wallpaper.toml")?;
        if legacy_config_file.exists() {
            legacy_config_file
        } else {
            xdg_dirs
                .place_config_file("config.toml")
                .context("unable to identify config file config.toml")?
        }
    };

    let reloaded = Arc::new(AtomicBool::new(false));
    // Do not stop when the configuration is invalid, we can always reload it at runtime
    let mut config = match Config::new_from_path(&config_file) {
        Ok(config) => config,
        Err(err) => {
            error!("{err:?}");
            let mut config = Config::default();
            config.path = config_file;
            config
        }
    };
    config.reloaded = Some(reloaded);

    // we use the OpenGL ES API because it's more widely supported
    // and it's used by wlroots
    egl.bind_api(egl::OPENGL_ES_API)
        .context("unable to select OpenGL API")?;

    let conn = Connection::connect_to_env()
        .context("connecting to wayland")
        .suggestion("Are you running a wayland compositor?")?;

    let egl_display = unsafe {
        egl.get_display(conn.display().id().as_ptr() as *mut std::ffi::c_void)
            .context("getting the display from the WlDisplay")?
    };
    egl.initialize(egl_display)
        .context("initializing the egl display")?;

    let (globals, event_queue) =
        registry_queue_init(&conn).context("initializing the wayland registry queue")?;
    let qh = event_queue.handle();

    let mut event_loop = calloop::EventLoop::<Wpaperd>::try_new()?;

    WaylandSource::new(conn.clone(), event_queue)
        .insert(event_loop.handle())
        .map_err(|e| anyhow!("insterting the wayland source into the event loop: {e}"))?;

    let (ping, ping_source) =
        calloop::ping::make_ping().context("Unable to create a calloop::ping::Ping")?;
    event_loop
        .handle()
        .insert_source(ping_source, |_, _, _| {})
        .map_err(|e| anyhow!("inserting the hotwatch event listener in the event loop: {e}"))?;

    let mut hotwatch = Hotwatch::new().context("hotwatch failed to initialize")?;
    config.listen_to_changes(&mut hotwatch, ping)?;

    let (ping, filelist_cache) =
        FilelistCache::new(config.paths(), &mut hotwatch, event_loop.handle())?;
    let filelist_cache = Rc::new(RefCell::new(filelist_cache));

    let groups = Rc::new(RefCell::new(WallpaperGroups::new()));

    let mut wpaperd = Wpaperd::new(
        &qh,
        &globals,
        config,
        egl_display,
        filelist_cache.clone(),
        groups,
    )?;

    // Start listening on the IPC socket
    let socket = listen_on_ipc_socket(&socket_path()?).context("spawning the ipc socket")?;

    // Add source to calloop loop.
    event_loop
        .handle()
        .insert_source(socket, |stream, _, wpaperd| {
            if let Err(err) = handle_message(stream, qh.clone(), wpaperd) {
                error!("{:?}", err);
            }
        })?;

    if let Some(notify) = opts.notify {
        let mut f = unsafe { File::from_raw_fd(notify as i32) };
        if let Err(err) = writeln!(f) {
            // This is not a hard error, just log it and go on
            error!("Could not write to FD {notify}: {err:?}");
        }
    }

    loop {
        // If the config has been modified, this value will return true
        if wpaperd
            .config
            .reloaded
            .as_ref()
            .unwrap()
            .load(Ordering::Acquire)
            && wpaperd.config.update()
        {
            // Update the filelist cache, keep it up to date
            // We need to call this before because updating the surfaces
            // will start loading the wallpapers in the background
            filelist_cache.borrow_mut().update_paths(
                wpaperd.config.paths(),
                &mut hotwatch,
                ping.clone(),
            );

            // Read the config, update the paths in the surfaces
            wpaperd.update_surfaces(event_loop.handle(), &qh);
        }

        // Due to how LayerSurface works, we cannot attach the egl window right away.
        // The LayerSurface needs to have received a configure callback first.
        // Afterwards we need to draw for the first time and then add a timer if needed.
        // We cannot use WlSurface::frame() because it only works for windows that are
        // already visible, hence we need to draw for the first time and then commit.
        wpaperd.surfaces.iter_mut().for_each(|surface| {
            if !surface.is_configured() {
                return;
            };

            // This is only true once per surface at startup (or when a new display gets connected)
            if !surface.has_been_drawn() {
                surface.add_timer(None, &event_loop.handle(), qh.clone());
                if let Err(err) = surface.draw(&qh, None) {
                    error!("{err:?}");
                };
                surface.drawn();
            } else {
                // If the surface has already been drawn for the first time, then handle pausing/resuming
                // the automatic wallpaper sequence.
                surface.handle_pause_state(&event_loop.handle(), qh.clone());
                if matches!(
                    surface.wallpaper_info.sorting,
                    Some(Sorting::GroupedRandom { .. })
                ) {
                    // surface.image_picker.handle_grouped_sorting();
                }
            };

            #[cfg(debug_assertions)]
            wpaperd.image_loader.borrow_mut().check_lingering_threads();
        });

        event_loop
            .dispatch(None, &mut wpaperd)
            .context("dispatching the event loop")?;
    }
}

fn main() -> Result<()> {
    color_eyre::install()?;

    let xdg_dirs = BaseDirectories::with_prefix("wpaperd")?;

    let opts = Opts::parse();

    let mut logger = Logger::try_with_env_or_str(if opts.verbose { "debug" } else { "info" })?;

    if opts.daemon {
        // If wpaperd detach, then log to files
        logger = logger.log_to_file(FileSpec::default().directory(xdg_dirs.get_state_home()));
        match unsafe { fork()? } {
            nix::unistd::ForkResult::Parent { child: _ } => exit(0),
            nix::unistd::ForkResult::Child => {}
        }
    } else {
        // otherwise prints everything in the stdout/stderr
        logger = logger.duplicate_to_stderr(Duplicate::Warn);
    }

    logger.start()?;

    if let Err(err) = run(opts, xdg_dirs) {
        error!("{err:?}");
        Err(err)
    } else {
        Ok(())
    }
}
