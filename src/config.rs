use std::path::PathBuf;

use clap::Parser;
use serde::{Deserialize, Serialize};

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
        long = "output-config",
        help = "Path to the configuration containing the outputs"
    )]
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
        help = "Draw the wallpaper as a scaled window. The compositor will upscale the wallpaper instead"
    )]
    #[serde(rename = "use-scaled-window")]
    pub use_scaled_window: bool,
}
