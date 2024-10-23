use std::{path::PathBuf, time::Duration};

use serde::Deserialize;

use crate::{image_picker::ImagePicker, render::Transition};

#[derive(PartialEq, Debug)]
pub struct WallpaperInfo {
    pub path: PathBuf,
    pub duration: Option<Duration>,
    pub apply_shadow: bool,
    pub sorting: Option<Sorting>,
    pub mode: BackgroundMode,
    pub drawn_images_queue_size: usize,
    pub transition_time: u32,

    /// Determines if we should show the transition between black and first
    /// wallpaper. `false` means we instantly cut to the first wallpaper,
    /// `true` means we fade from black to the first wallpaper.
    pub initial_transition: bool,
    pub transition: Transition,

    /// Determine the offset for the wallpaper to be drawn into the screen
    /// Must be from 0.0 to 1.0, by default is 0.0 in tile mode and 0.5 in all the others
    pub offset: Option<f32>,
}

impl Default for WallpaperInfo {
    fn default() -> Self {
        Self {
            path: PathBuf::new(),
            duration: None,
            apply_shadow: false,
            sorting: None,
            mode: BackgroundMode::default(),
            drawn_images_queue_size: ImagePicker::DEFAULT_DRAWN_IMAGES_QUEUE_SIZE,
            transition_time: Transition::Fade {}.default_transition_time(),
            initial_transition: true,
            transition: Transition::Fade {},
            offset: None,
        }
    }
}

#[derive(Debug, Copy, Clone, Default, Eq, PartialEq)]
pub enum Sorting {
    #[default]
    Random,
    GroupedRandom {
        group: u8,
    },
    Ascending,
    Descending,
}

#[derive(Debug, Copy, Clone, Default, Eq, PartialEq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BackgroundMode {
    Stretch,
    #[default]
    Center,
    Fit,
    Tile,
    FitBorderColor,
}
