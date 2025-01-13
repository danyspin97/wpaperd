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
use color_eyre::eyre::{eyre, OptionExt, WrapErr};
use color_eyre::{Result, Section};
use config::Config;
use egl::API as egl;
use filelist_cache::FilelistCache;
use flexi_logger::{Duplicate, FileSpec, Logger};
use hotwatch::Hotwatch;
use image_loader::ImageLoader;
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

#[cfg(feature = "jemalloc")]
use tikv_jemallocator::Jemalloc;

#[cfg(feature = "jemalloc")]
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
            .wrap_err("Failed to locate legacy config file wallpaper.toml")?;
        if legacy_config_file.exists() {
            legacy_config_file
        } else {
            xdg_dirs
                .place_config_file("config.toml")
                .wrap_err("Failed to locate config file config.toml")?
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
        .wrap_err("Failed to bind OpenGL ES API during initialization")?;

    let conn = Connection::connect_to_env()
        .wrap_err("Failed to connect to the Wayland server")
        .suggestion("Are you running a wayland compositor?")?;

    let egl_display = unsafe {
        egl.get_display(conn.display().id().as_ptr() as *mut std::ffi::c_void)
            .ok_or_eyre("Failed to get EGL display during initialization")?
    };
    egl.initialize(egl_display)
        .wrap_err("Failed the EGL display initialization")?;

    let (globals, event_queue) =
        registry_queue_init(&conn).wrap_err("Failed to initialize the Wayland registry queue")?;
    let qh = event_queue.handle();

    let mut event_loop = calloop::EventLoop::<Wpaperd>::try_new()?;

    WaylandSource::new(conn.clone(), event_queue)
        .insert(event_loop.handle())
        .map_err(|e| eyre!("{e}"))
        .wrap_err("Failed to insert the Wayland source into the event loop")?;

    let (ping, ping_source) = calloop::ping::make_ping()
        .wrap_err("Failed to create a calloop::ping::Ping for the hotwatch listener")?;
    event_loop
        .handle()
        .insert_source(ping_source, |_, _, _| {})
        .map_err(|e| eyre!("{e}"))
        .wrap_err("Failed to insert the hotwatch listener into the event loop")?;

    let mut hotwatch = Hotwatch::new().wrap_err("Failed to initialize hotwatch listener")?;
    config
        .listen_to_changes(&mut hotwatch, ping)
        .wrap_err("Failed to watch on config file changes")?;

    let (ping, filelist_cache) =
        FilelistCache::new(config.paths(), &mut hotwatch, event_loop.handle())
            .wrap_err("Failed to create FilelistCache")?;
    let filelist_cache = Rc::new(RefCell::new(filelist_cache));

    let groups = Rc::new(RefCell::new(WallpaperGroups::new()));

    let (image_loader_ping, ping_source) = calloop::ping::make_ping()
        .wrap_err("Failed to create a calloop::ping::Ping for the image loader")?;
    let handle = event_loop.handle();
    let qh_clone = qh.clone();
    event_loop
        .handle()
        .insert_source(ping_source, move |_, _, wpaperd| {
            // An image has been loaded, update the surfaces status
            wpaperd.surfaces.iter_mut().for_each(|surface| {
                match surface.load_wallpaper(Some(&handle)) {
                    Ok(wallpaper_loaded) => {
                        if wallpaper_loaded {
                            surface.queue_draw(&qh_clone);
                            surface.image_picker.handle_grouped_sorting(&qh_clone);
                        }
                    }

                    Err(err) => error!("{err:?}"),
                }
            });
        })
        .map_err(|e| eyre!("{e}"))
        .wrap_err("Failed to insert the image loader listener into the event loop")?;
    let image_loader = Rc::new(RefCell::new(ImageLoader::new(image_loader_ping)));

    let mut wpaperd = Wpaperd::new(
        &qh,
        &globals,
        config,
        egl_display,
        filelist_cache.clone(),
        groups,
        image_loader,
        xdg_dirs,
    )
    .wrap_err("Failed to initiliaze wpaperd status")?;

    // Start listening on the IPC socket
    let socket = listen_on_ipc_socket(&socket_path().wrap_err("Failed to locate wpaperd socket")?)
        .wrap_err("Failed to listen to IPC socket")?;

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

    let (ctrlc_ping, ctrl_ping_source) =
        calloop::ping::make_ping().wrap_err("Failed to create calloop::")?;
    let should_exit = Arc::new(AtomicBool::new(false));
    let should_exit_clone = should_exit.clone();
    // Handle SIGINT, SIGTERM, and SIGHUP, so that the application can stop nicely
    ctrlc::set_handler(move || {
        // Just wake up the event loop. The actual exit will be handled by the main loop
        // The event loop callback will set should_exit to true
        ctrlc_ping.ping();
    })
    .wrap_err("Failed to set signal handler")?;
    event_loop
        .handle()
        .insert_source(ctrl_ping_source, move |_, _, _| {
            should_exit_clone.store(true, Ordering::Release);
        })
        .map_err(|e| eyre!("{e}"))
        .wrap_err("Failed to insert the signal handler listener into the event loop")?;

    loop {
        if should_exit.load(Ordering::Acquire) {
            break Ok(());
        }

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
                // Add the first timer, it will run endlessy or it will be updated in
                // Surface::handle_new_duration
                surface.add_timer(&event_loop.handle(), qh.clone(), None);
                if surface.try_drawing(&qh, None) {
                    surface.drawn();
                }
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
            .wrap_err("Failed to dispatch the event loop")?;
    }
}

fn main() -> Result<()> {
    color_eyre::install().wrap_err("Failed to inject color_eyre")?;

    let xdg_dirs =
        BaseDirectories::with_prefix("wpaperd").wrap_err("Failed to initialize XDG directories")?;

    let opts = Opts::parse();

    let mut logger = Logger::try_with_env_or_str(if opts.verbose { "debug" } else { "info" })
        .wrap_err("Failed to initialize logger")?;

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

    logger.start().wrap_err("Failed to start logger")?;

    if let Err(err) = run(opts, xdg_dirs) {
        error!("{err:?}");
        Err(err)
    } else {
        Ok(())
    }
}
