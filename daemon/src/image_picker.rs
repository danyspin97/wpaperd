use std::{path::PathBuf, sync::Arc, time::Instant};

use color_eyre::eyre::{bail, ensure, Context};
use image::{open, DynamicImage};
use log::{info, warn};
use walkdir::WalkDir;

use crate::wallpaper_info::{Sorting, WallpaperInfo};

pub struct ImagePicker {
    current_img: PathBuf,
    current_index: usize,
    drawn_images: Vec<PathBuf>,
    pub wallpaper_info: Arc<WallpaperInfo>,
    pub image_changed_instant: Instant,
}

impl ImagePicker {
    pub fn new(wallpaper_info: Arc<WallpaperInfo>) -> Self {
        Self {
            current_img: PathBuf::new(),
            current_index: 0,
            drawn_images: Vec::new(),
            wallpaper_info,
            image_changed_instant: Instant::now(),
        }
    }

    /// Get index for the next image based on the sorting method
    fn get_new_image_index(&self, files: &Vec<PathBuf>) -> usize {
        match self.wallpaper_info.sorting {
            Sorting::Random => rand::random::<usize>() % files.len(),
            Sorting::Ascending => {
                let idx = match files.binary_search(&self.current_img) {
                    // Perform increment here, do validation/bounds checking below
                    Ok(n) => n + 1,
                    Err(err) => {
                        info!(
                            "Current image not found, defaulting to first image ({:?})",
                            err
                        );
                        // set idx to > slice length so the guard sets it correctly for us
                        files.len()
                    }
                };

                if idx >= files.len() {
                    0
                } else {
                    idx
                }
            }
            Sorting::Descending => {
                let idx = match files.binary_search(&self.current_img) {
                    Ok(n) => n,
                    Err(err) => {
                        info!(
                            "Current image not found, defaulting to last image ({:?})",
                            err
                        );
                        files.len()
                    }
                };

                // Here, bounds checking is strictly ==, as we cannot go lower than 0 for usize
                if idx == 0 {
                    files.len() - 1
                } else {
                    idx - 1
                }
            }
        }
    }

    fn get_image_files_from_dir(&self, dir_path: &PathBuf) -> Vec<PathBuf> {
        WalkDir::new(dir_path)
            .sort_by_file_name()
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| {
                if let Some(guess) = new_mime_guess::from_path(e.path()).first() {
                    guess.type_() == "image"
                } else {
                    false
                }
            })
            .map(|e| e.path().to_path_buf())
            .collect()
    }

    pub fn get_image(&mut self) -> Result<DynamicImage, color_eyre::Report> {
        let path = self.wallpaper_info.path.as_ref().unwrap();
        let mut tries = 0;
        if path.is_dir() {
            loop {
                let files = self.get_image_files_from_dir(path);

                // There are no images, forcefully break out of the loop
                if files.is_empty() {
                    bail!("Directory {path:?} does not contain any valid image files.");
                }

                let is_below_len =
                    !self.drawn_images.is_empty() && self.current_index < self.drawn_images.len();

                let img_path = if is_below_len && self.drawn_images[self.current_index].is_file() {
                    self.drawn_images[self.current_index].clone()
                } else {
                    // Select new image based on sorting method
                    let index = self.get_new_image_index(&files);
                    files[index].clone()
                };

                log::trace!("{img_path:?}");

                match open(&img_path).with_context(|| format!("opening the image {img_path:?}")) {
                    Ok(image) => {
                        // TODO
                        // info!("New image for monitor {:?}: {img_path:?}", self.name());

                        if !self.drawn_images.contains(&img_path) {
                            self.drawn_images.push(img_path.clone());
                            self.current_index = self.drawn_images.len() - 1;
                        };

                        self.current_img = img_path;

                        break Ok(image);
                    }
                    Err(err) => {
                        warn!("{err:?}");
                        tries += 1;
                    }
                };

                ensure!(
                    tries < 5,
                    "tried reading an image from the directory {path:?} without success",
                );
            }
        } else {
            open(path).with_context(|| format!("opening the image {:?}", &path))
        }
    }

    /// Update wallpaper by going down 1 index through the cached image paths
    /// Expiry timer reset even if already at the first cached image
    pub fn previous_image(&mut self) {
        if self.current_index == 0 {
            return;
        };

        self.image_changed_instant = Instant::now();

        self.current_index -= 1;
    }

    /// Update wallpaper by going up 1 index through the cached image paths
    pub fn next_image(&mut self) {
        self.image_changed_instant = Instant::now();
        if self.current_index > self.drawn_images.len() {
            return;
        };

        self.current_index += 1;
    }

    pub fn current_image(&self) -> PathBuf {
        self.current_img.clone()
    }

    pub fn apply_shadow(&self) -> bool {
        self.wallpaper_info.apply_shadow.unwrap_or(false)
    }
}
