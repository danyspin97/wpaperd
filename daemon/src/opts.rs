use std::path::PathBuf;

use clap::Parser;

#[derive(Parser)]
#[clap(
    author = "Danilo Spinella <danilo.spinella@suse.com>",
    version,
    about = "A wallpaper manager for Wayland compositors"
)]
pub struct Opts {
    #[clap(
        action,
        short,
        long,
        help = "Path to the configuration (XDG_CONFIG_HOME/wpaperd/config.toml by default)"
    )]
    pub config: Option<PathBuf>,
    #[clap(
        action,
        short,
        long,
        help = "Detach from the current terminal and run in the background"
    )]
    pub daemon: bool,
    #[clap(short, long, help = "Increase the verbosity of wpaperd")]
    pub verbose: bool,
    #[clap(
        long,
        help = "Readiness fd used by wpaperd to signal that it has started correctly"
    )]
    pub notify: Option<u8>,
}
