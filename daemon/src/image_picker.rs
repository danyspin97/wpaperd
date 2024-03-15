use std::{
    cell::RefCell,
    path::{Path, PathBuf},
    rc::Rc,
    time::Instant,
};

use color_eyre::eyre::{bail, ensure, Context};
use image::{open, DynamicImage};
use log::warn;

use crate::{
    filelist_cache::FilelistCache,
    wallpaper_info::{Sorting, WallpaperInfo},
};

#[derive(Debug)]
enum ImagePickerAction {
    Next,
    Previous,
}

#[derive(Debug)]
enum ImagePickerSorting {
    Random {
        drawn_images: Vec<PathBuf>,
        tail: usize,
        current: usize,
    },
    Ascending(usize),
    Descending(usize),
}

impl ImagePickerSorting {
    const RANDOM_DEFAULT_SIZE: usize = 10;
    fn new_random() -> Self {
        ImagePickerSorting::Random {
            drawn_images: Vec::with_capacity(Self::RANDOM_DEFAULT_SIZE),
            tail: 0,
            current: Self::RANDOM_DEFAULT_SIZE - 1,
        }
    }
}

pub struct ImagePicker {
    current_img: PathBuf,
    pub image_changed_instant: Instant,
    action: Option<ImagePickerAction>,
    sorting: ImagePickerSorting,
    filelist_cache: Rc<RefCell<FilelistCache>>,
}

impl ImagePicker {
    pub fn new(wallpaper_info: &WallpaperInfo, filelist_cache: Rc<RefCell<FilelistCache>>) -> Self {
        Self {
            current_img: PathBuf::from(""),
            image_changed_instant: Instant::now(),
            action: Some(ImagePickerAction::Next),
            sorting: match wallpaper_info.sorting {
                Sorting::Random => ImagePickerSorting::new_random(),
                Sorting::Ascending => ImagePickerSorting::Ascending(usize::MAX),
                Sorting::Descending => ImagePickerSorting::Descending(usize::MAX),
            },
            filelist_cache,
        }
    }

    /// Get the next image based on the sorting method
    fn get_image_path(&mut self, files: &[PathBuf]) -> (usize, PathBuf) {
        match (&self.action, &mut self.sorting) {
            (None, _) if self.current_img.exists() => unreachable!(),
            (
                None | Some(ImagePickerAction::Next),
                ImagePickerSorting::Random {
                    drawn_images,
                    tail,
                    current,
                },
            ) if (*current + 1) % drawn_images.capacity() == *tail => {
                let mut tries = 5;
                loop {
                    let index = rand::random::<usize>() % files.len();
                    // search for an image that has not been drawn yet
                    // fail after 5 tries
                    if tries == 0 || !drawn_images.contains(&files[index]) {
                        break (index, files[index].to_path_buf());
                    }

                    tries -= 1;
                }
            }
            (
                None | Some(ImagePickerAction::Next),
                ImagePickerSorting::Random {
                    drawn_images,
                    tail: _,
                    current,
                },
            ) => {
                *current = (*current + 1) % drawn_images.capacity();
                (*current, drawn_images[*current].clone())
            }
            (
                Some(ImagePickerAction::Previous),
                ImagePickerSorting::Random {
                    drawn_images,
                    tail,
                    current,
                },
            ) if current == tail
                || (drawn_images.len() != drawn_images.capacity() && *current == 0) =>
            {
                (usize::MAX, self.current_image())
            }
            (
                Some(ImagePickerAction::Previous),
                ImagePickerSorting::Random {
                    drawn_images,
                    tail,
                    current,
                },
            ) if drawn_images.len() == drawn_images.capacity() => {
                let mut i = *current;
                loop {
                    i = (i + drawn_images.capacity() - 1) % drawn_images.capacity();
                    let path = &drawn_images[i];
                    if path.exists() {
                        // we update here in case the image could not be read and we want to start
                        // from this index next time
                        *current = i;
                        break (i, path.clone());
                    }

                    // this is the last image
                    if i == *tail {
                        break (*current, self.current_image());
                    }
                }
            }
            (
                Some(ImagePickerAction::Previous),
                ImagePickerSorting::Random {
                    drawn_images,
                    tail,
                    current,
                },
            ) => drawn_images
                .iter()
                .enumerate()
                .rev()
                .skip(*tail - *current)
                .find(|(_, img)| img.exists())
                .map(|(i, img)| {
                    *current = i;
                    (i, img.clone())
                })
                .unwrap_or_else(|| (*current, self.current_img.clone())),
            // The current image is still in the same place
            (Some(ImagePickerAction::Next), ImagePickerSorting::Descending(current_index))
            | (Some(ImagePickerAction::Previous), ImagePickerSorting::Ascending(current_index))
                if files.get(*current_index) == Some(&self.current_img) =>
            {
                // Start from the end
                files
                    .get(*current_index - 1)
                    .map(|p| (*current_index - 1, p.to_path_buf()))
                    .unwrap_or_else(|| {
                        (
                            files.len(),
                            files
                                .last()
                                .expect("files vec to not be empty")
                                .to_path_buf(),
                        )
                    })
            }
            // The image index is different
            (
                None | Some(ImagePickerAction::Next),
                ImagePickerSorting::Descending(current_index),
            )
            | (
                None | Some(ImagePickerAction::Previous),
                ImagePickerSorting::Ascending(current_index),
            ) => match files.binary_search(&self.current_img) {
                Ok(new_index) => files
                    .get(new_index - 1)
                    .map(|p| (new_index - 1, p.to_path_buf()))
                    .unwrap_or_else(|| (files.len(), files.last().unwrap().to_path_buf())),
                Err(_err) => files
                    .get(*current_index - 1)
                    .map(|p| (*current_index - 1, p.to_path_buf()))
                    .unwrap_or_else(|| (files.len(), files.last().unwrap().to_path_buf())),
            },
            // The current image is still in the same place
            (Some(ImagePickerAction::Previous), ImagePickerSorting::Descending(current_index))
            | (Some(ImagePickerAction::Next), ImagePickerSorting::Ascending(current_index))
                if files.get(*current_index) == Some(&self.current_img) =>
            {
                // Start from the end
                files
                    .get(*current_index + 1)
                    .map(|p| (*current_index + 1, p.to_path_buf()))
                    .unwrap_or_else(|| (0, files.first().unwrap().to_path_buf()))
            }
            // The image index is different
            (Some(ImagePickerAction::Previous), ImagePickerSorting::Descending(current_index))
            | (Some(ImagePickerAction::Next), ImagePickerSorting::Ascending(current_index)) => {
                match files.binary_search(&self.current_img) {
                    Ok(new_index) => files
                        .get(new_index + 1)
                        .map(|p| (new_index + 1, p.to_path_buf()))
                        .unwrap_or_else(|| (0, files.first().unwrap().to_path_buf())),
                    Err(_err) => files
                        .get(*current_index + 1)
                        .map(|p| (*current_index + 1, p.to_path_buf()))
                        .unwrap_or_else(|| (0, files.first().unwrap().to_path_buf())),
                }
            }
        }
    }

    pub fn get_image_from_path(
        &mut self,
        path: &Path,
    ) -> Result<Option<DynamicImage>, color_eyre::Report> {
        if path.is_dir() {
            if self.action.is_none() {
                return Ok(None);
            }

            let mut tries = 0;
            loop {
                let files = self.filelist_cache.borrow().get(path);

                // There are no images, forcefully break out of the loop
                if files.is_empty() {
                    bail!("Directory {path:?} does not contain any valid image files.");
                }

                let (index, img_path) = self.get_image_path(&files);
                if img_path == self.current_img {
                    break Ok(None);
                }
                match open(&img_path).with_context(|| format!("opening the image {img_path:?}")) {
                    Ok(image) => {
                        match (self.action.take(), &mut self.sorting) {
                            (
                                Some(ImagePickerAction::Next),
                                ImagePickerSorting::Random {
                                    drawn_images,
                                    tail,
                                    current,
                                },
                                // if the current image is the last one in the list
                            ) if (*current + 1) % drawn_images.capacity() == *tail => {
                                // Use drawn_images as a circular buffer
                                if drawn_images.len() == drawn_images.capacity() {
                                    debug_assert!(tail != current);
                                    *current = (*current + 1) % drawn_images.len();
                                    drawn_images[*current] = img_path.clone();
                                    if current == tail {
                                        *tail = (*tail + 1) % drawn_images.capacity();
                                    }
                                } else {
                                    drawn_images.push(img_path.clone());
                                    *current = *tail;
                                    *tail = (*tail + 1) % drawn_images.capacity();
                                }
                            }
                            (Some(ImagePickerAction::Next), ImagePickerSorting::Random { .. }) => {}
                            (
                                None | Some(ImagePickerAction::Previous),
                                ImagePickerSorting::Random { .. },
                            ) => {}
                            (
                                _,
                                ImagePickerSorting::Ascending(current_index)
                                | ImagePickerSorting::Descending(current_index),
                            ) => *current_index = index,
                        }

                        self.current_img = img_path;

                        break Ok(Some(image));
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
        } else if path == self.current_img {
            Ok(None)
        } else {
            // path is not a directory and it's not the current image
            // try open it and update the current image accordingly
            match open(path).with_context(|| format!("opening the image {:?}", &path)) {
                Ok(image) => {
                    self.current_img = path.to_path_buf();
                    Ok(Some(image))
                }
                Err(err) => Err(err),
            }
        }
    }

    /// Update wallpaper by going down 1 index through the cached image paths
    /// Expiry timer reset even if already at the first cached image
    pub fn previous_image(&mut self) {
        self.action = Some(ImagePickerAction::Previous);
    }

    /// Update wallpaper by going up 1 index through the cached image paths
    pub fn next_image(&mut self) {
        self.action = Some(ImagePickerAction::Next);
    }

    pub fn current_image(&self) -> PathBuf {
        self.current_img.clone()
    }

    /// Return true if the path changed
    pub fn update_sorting(&mut self, new_sorting: Sorting, path_changed: bool) {
        match (&mut self.sorting, new_sorting) {
            (
                ImagePickerSorting::Random { .. } | ImagePickerSorting::Descending(_),
                Sorting::Ascending,
            ) => self.sorting = ImagePickerSorting::Ascending(usize::MAX),
            (
                ImagePickerSorting::Random { .. } | ImagePickerSorting::Ascending(_),
                Sorting::Descending,
            ) => self.sorting = ImagePickerSorting::Descending(usize::MAX),
            (
                ImagePickerSorting::Descending(_) | ImagePickerSorting::Ascending(_),
                Sorting::Random,
            ) if path_changed => {
                // If the path was changed, use a new random sorting
                self.sorting = ImagePickerSorting::new_random();
            }
            (
                ImagePickerSorting::Descending(_) | ImagePickerSorting::Ascending(_),
                Sorting::Random,
            ) => {
                // if the path was not changed, use the current image as the first image of
                // the drawn_images
                self.sorting = ImagePickerSorting::Random {
                    drawn_images: {
                        let mut v = Vec::with_capacity(ImagePickerSorting::RANDOM_DEFAULT_SIZE);
                        v.push(self.current_img.clone());
                        v
                    },
                    tail: 1,
                    current: 0,
                };
            }
            // The path has changed, use a new random sorting, otherwise we reuse the current
            // drawn_images
            (ImagePickerSorting::Random { .. }, Sorting::Random) if path_changed => {
                self.sorting = ImagePickerSorting::new_random();
            }
            // No need to update the sorting if it's the same
            (_, _) => {}
        }
    }
}
