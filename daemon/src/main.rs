mod config;
mod filelist_cache;
mod image_picker;
mod ipc_server;
mod render;
mod socket;
mod surface;
mod wallpaper_config;
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
use color_eyre::{eyre::WrapErr, Result};
use egl::API as egl;
use figment::{
    providers::{Format, Serialized, Toml},
    Figment,
};
use filelist_cache::FilelistCache;
use flexi_logger::{Duplicate, FileSpec, Logger};
use hotwatch::Hotwatch;
use log::error;
use nix::unistd::fork;
use smithay_client_toolkit::reexports::{
    calloop,
    calloop_wayland_source::WaylandSource,
    client::{globals::registry_queue_init, Connection, Proxy},
};
use wpaperd_ipc::socket_path;
use xdg::BaseDirectories;

use crate::config::Config;
use crate::wallpaper_config::WallpapersConfig;
use crate::wpaperd::Wpaperd;

fn run(config: Config, xdg_dirs: BaseDirectories) -> Result<()> {
    // Path passed from the CLI or the wpaperd.toml file has precedence
    let wallpaper_config_file = if let Some(wallpaper_config) = config.wallpaper_config {
        wallpaper_config
    } else {
        // Read the new config or the legacy file
        let legacy_config_file = xdg_dirs.place_config_file("output.conf").unwrap();
        if legacy_config_file.exists() {
            legacy_config_file
        } else {
            xdg_dirs.place_config_file("wallpaper.toml").unwrap()
        }
    };

    let reloaded = Arc::new(AtomicBool::new(false));
    let mut wallpaper_config = WallpapersConfig::new_from_path(&wallpaper_config_file)?;
    wallpaper_config.reloaded = Some(reloaded);

    // we use the OpenGL ES API because it's more widely supported
    // and it's used by wlroots
    egl.bind_api(egl::OPENGL_ES_API)
        .expect("unable to select OpenGL API");

    let conn = Connection::connect_to_env().unwrap();

    let egl_display = unsafe {
        egl.get_display(conn.display().id().as_ptr() as *mut std::ffi::c_void)
            .unwrap()
    };
    egl.initialize(egl_display).unwrap();

    let (globals, event_queue) = registry_queue_init(&conn).unwrap();
    let qh = event_queue.handle();

    let mut event_loop = calloop::EventLoop::<Wpaperd>::try_new()?;

    WaylandSource::new(conn.clone(), event_queue)
        .insert(event_loop.handle())
        .unwrap();

    let (ev_tx, ev_rx) = calloop::channel::channel();
    event_loop
        .handle()
        .insert_source(ev_rx, |_, _, _| {})
        .unwrap();

    let mut hotwatch = Hotwatch::new().context("hotwatch failed to initialize")?;
    wallpaper_config.listen_to_changes(&mut hotwatch, ev_tx)?;

    let filelist_cache = Rc::new(RefCell::new(FilelistCache::new()));
    filelist_cache
        .borrow_mut()
        .update_paths(wallpaper_config.paths(), &mut hotwatch);

    let mut wpaperd = Wpaperd::new(
        &qh,
        &globals,
        wallpaper_config,
        egl_display,
        filelist_cache.clone(),
    )?;

    loop {
        let all_configured = !wpaperd.surfaces.is_empty()
            && wpaperd.surfaces.iter_mut().all(|surface| {
                // We need to add the first timer here, so that in the next
                // loop we will always receive timeout events and create
                // them when that happens
                if surface.is_configured() {
                    surface.add_timer(None, &event_loop.handle(), qh.clone());
                    if let Err(err) = surface.draw(&qh, 0) {
                        log::error!("{err}");
                    }
                    true
                } else {
                    false
                }
            });

        // Break to the actual event_loop
        if all_configured {
            break;
        }

        event_loop
            .dispatch(None, &mut wpaperd)
            .context("dispatching the event loop")?;
    }

    ipc_server::spawn_ipc_socket(&socket_path()?, &event_loop.handle(), qh.clone()).unwrap();
    if let Some(notify) = config.notify {
        let mut f = unsafe { File::from_raw_fd(notify as i32) };
        if let Err(err) = writeln!(f) {
            error!("Could not write to FD {notify}: {err}");
        }
    }

    loop {
        // If wallpaper_config.toml has been modified, this value will return true
        if wpaperd
            .wallpaper_config
            .reloaded
            .as_ref()
            .unwrap()
            .load(Ordering::Acquire)
        {
            // Read the config, update the paths in the surfaces
            wpaperd.update_wallpaper_config(event_loop.handle(), &qh);

            // Update the filelist cache as well, keep it up to date
            filelist_cache
                .borrow_mut()
                .update_paths(wpaperd.wallpaper_config.paths(), &mut hotwatch);
        }

        event_loop
            .dispatch(None, &mut wpaperd)
            .context("dispatching the event loop")?;
    }
}

fn main() -> Result<()> {
    color_eyre::install()?;

    let xdg_dirs = BaseDirectories::with_prefix("wpaperd")?;

    let mut config = Figment::new();
    let opts = Config::parse();

    if let Some(opts_config) = &opts.config {
        config = config.merge(Toml::file(opts_config));
    } else {
        // Otherwise read the new config or the legacy file
        let legacy_config = xdg_dirs.place_config_file("wpaperd.conf").unwrap();
        if legacy_config.exists() {
            config = config.merge(Toml::file(legacy_config));
        } else {
            config = config.merge(Toml::file(
                xdg_dirs.place_config_file("wpaperd.toml").unwrap(),
            ))
        }
    }

    let config: Config = config.merge(Serialized::defaults(opts)).extract()?;

    let mut logger = Logger::try_with_env_or_str(if config.verbose { "info" } else { "warn" })?;

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

    if let Err(err) = run(config, xdg_dirs) {
        error!("{err:?}");
        Err(err)
    } else {
        Ok(())
    }
}
