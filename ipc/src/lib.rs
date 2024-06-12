use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use xdg::{BaseDirectories, BaseDirectoriesError};

#[derive(Serialize, Deserialize)]
pub enum IpcMessage {
    CurrentWallpaper { monitor: String },
    NextWallpaper { monitors: Vec<String> },
    PreviousWallpaper { monitors: Vec<String> },
    PauseWallpaper { monitors: Vec<String> },
    ResumeWallpaper { monitors: Vec<String> },
    AllWallpapers,
    ReloadWallpaper { monitors: Vec<String> },
    SetWallpaper { monitor: String, wallpaper: PathBuf },
}

#[derive(Serialize, Deserialize)]
pub enum IpcResponse {
    CurrentWallpaper { path: PathBuf },
    AllWallpapers { entries: Vec<(String, PathBuf)> },
    Ok,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum IpcError {
    MonitorNotFound { monitor: String },
    DrawErrors(Vec<(String, String)>),
}

pub fn socket_path() -> Result<PathBuf, BaseDirectoriesError> {
    let xdg_dirs = BaseDirectories::with_prefix("wpaperd")?;
    Ok(xdg_dirs.get_runtime_directory()?.join("wpaperd.sock"))
}
