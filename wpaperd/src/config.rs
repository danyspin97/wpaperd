use std::path::PathBuf;

use clap::Parser;
use serde::Deserialize;

#[derive(Parser, Deserialize)]
#[clap(
    author = "Danilo Spinella <danilo.spinella@suse.com>",
    version,
    about = "A wallpaper manager for Wayland compositors"
)]
pub struct Config {
    #[clap(short, long, help = "Path to the configuration to read from")]
    #[serde(skip)]
    pub config: Option<PathBuf>,
    #[clap(
        short,
        long = "output-config",
        help = "Path to the configuration containing the outputs"
    )]
    pub output_config: Option<PathBuf>,
    #[clap(
        short = 'n',
        long = "no-daemon",
        help = "Stay in foreground, do not detach"
    )]
    #[serde(rename = "no-daemon")]
    pub no_daemon: bool,
}

impl Config {
    pub fn merge(&mut self, o: Self) {
        if let Some(output_config) = o.output_config {
            self.output_config = Some(output_config);
        }

        self.no_daemon |= o.no_daemon;
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            config: None,
            output_config: None,
            no_daemon: false,
        }
    }
}
