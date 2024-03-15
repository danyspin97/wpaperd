use std::{path::PathBuf, time::Duration};

use serde::Deserialize;

#[derive(PartialEq, Debug, Default)]
pub struct WallpaperInfo {
    pub path: PathBuf,
    pub duration: Option<Duration>,
    pub apply_shadow: bool,
    pub sorting: Sorting,
    pub mode: BackgroundMode,
}

#[derive(Debug, Copy, Clone, Default, Eq, PartialEq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Sorting {
    #[default]
    Random,
    Ascending,
    Descending,
}

#[derive(Debug, Copy, Clone, Default, Eq, PartialEq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BackgroundMode {
    Stretch,
    #[default]
    Fill,
    Fit,
    Tile,
}
