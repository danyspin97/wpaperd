use clap::Parser;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
pub struct Opts {
    #[clap(subcommand)]
    pub subcmd: SubCmd,
}

#[derive(clap::Subcommand)]
pub enum SubCmd {
    #[clap(visible_alias = "get")]
    GetWallpaper { monitor: String },
    #[clap(visible_alias = "get-all")]
    AllWallpapers {
        #[clap(short, long)]
        json: bool,
    },
    #[clap(visible_alias = "next")]
    NextWallpaper { monitors: Vec<String> },
    #[clap(visible_alias = "previous")]
    PreviousWallpaper { monitors: Vec<String> },
    #[clap(visible_alias = "reload")]
    ReloadWallpaper { monitors: Vec<String> },
    #[clap(visible_alias = "pause")]
    PauseWallpaper { monitors: Vec<String> },
    #[clap(visible_alias = "resume")]
    ResumeWallpaper { monitors: Vec<String> },

    #[clap(visible_alias = "set")]
    SetWallaper {
        monitor: String,
        wallpaper: std::path::PathBuf,
    },
}
