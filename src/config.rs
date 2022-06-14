use std::path::PathBuf;

use clap::Parser;
use serde::Deserialize;

#[derive(Default, Parser, Deserialize)]
#[clap(
    author = "Danilo Spinella <danilo.spinella@suse.com>",
    version,
    about = "A wallpaper manager for Wayland compositors"
)]
pub struct Config {
    #[clap(action, short, long, help = "Path to the configuration to read from")]
    #[serde(skip)]
    pub config: Option<PathBuf>,
    #[clap(
        action,
        short,
        long = "output-config",
        help = "Path to the configuration containing the outputs"
    )]
    #[serde(rename = "output-config")]
    pub output_config: Option<PathBuf>,
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

impl Config {
    pub fn merge(&mut self, o: Self) {
        if let Some(output_config) = o.output_config {
            self.output_config = Some(output_config);
        }

        self.no_daemon |= o.no_daemon;
    }
}
