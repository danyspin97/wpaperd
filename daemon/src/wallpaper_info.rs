use std::{path::PathBuf, time::Duration};

use serde::Deserialize;

use crate::image_picker::ImagePicker;

#[derive(PartialEq, Debug)]
pub struct WallpaperInfo {
    pub path: PathBuf,
    pub duration: Option<Duration>,
    pub apply_shadow: bool,
    pub sorting: Sorting,
    pub mode: BackgroundMode,
    pub drawn_images_queue_size: usize,
}

impl Default for WallpaperInfo {
    fn default() -> Self {
        Self {
            path: PathBuf::new(),
            duration: None,
            apply_shadow: false,
            sorting: Sorting::default(),
            mode: BackgroundMode::default(),
            drawn_images_queue_size: ImagePicker::DEFAULT_DRAWN_IMAGES_QUEUE_SIZE,
        }
    }
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
