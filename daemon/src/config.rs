use std::path::PathBuf;

use clap::{Parser, ValueEnum};
use serde::{Deserialize, Serialize};

// Use nearest as default in debug builds
// use triangle as default in triangle
#[derive(Default, ValueEnum, Serialize, Deserialize, Clone)]
pub enum FilterType {
    #[cfg(debug)]
    #[default]
    Nearest,
    #[cfg(debug)]
    Triangle,
    #[cfg(not(debug))]
    Nearest,
    #[cfg(not(debug))]
    #[default]
    Triangle,
    CatmullRom,
    Gaussian,
    Lanczos3,
}

#[derive(Default, Parser, Serialize, Deserialize)]
#[clap(
    author = "Danilo Spinella <danilo.spinella@suse.com>",
    version,
    about = "A wallpaper manager for Wayland compositors"
)]
pub struct Config {
    #[clap(
        action,
        short,
        long,
        help = "Path to the configuration (XDG_CONFIG_HOME/wpaperd/wpaperd.toml by default)"
    )]
    #[serde(skip)]
    pub config: Option<PathBuf>,
    #[clap(
        action,
        short,
        long = "wallpaper-config",
        help = "Path to the configuration of the wallpaper (XDG_CONFIG_HOME/wpaperd/wallpaper.toml by default)"
    )]
    pub wallpaper_config: Option<PathBuf>,
    #[clap(
        action,
        short = 'n',
        long = "no-daemon",
        help = "Stay in foreground, do not detach"
    )]
    #[serde(rename = "no-daemon")]
    pub no_daemon: bool,
    #[clap(
        action,
        long = "use-scaled-window",
        help = "DEPRECATED: Draw the wallpaper as a scaled window. The compositor will upscale the wallpaper instead"
    )]
    #[serde(rename = "use-scaled-window")]
    pub use_scaled_window: bool,
    #[clap(
        action,
        long,
        help = concat!("Draw the wallpaper as a window with native resolution.",
                       " By default the window at the resolution u` ")
    )]
    #[serde(rename = "use-native-resolution")]
    pub use_native_resolution: bool,
    #[clap(short, long, help = "Increase the verbosity of wpaperd")]
    pub verbose: bool,
    #[clap(
        long,
        help = "Fd to write once wpaperd is running (used for readiness)"
    )]
    pub notify: Option<u8>,
    #[clap(
        long,
        default_value = "triangle",
        help = "Decide the sampling filter to use"
    )]
    pub sampling_filter: FilterType,
}
