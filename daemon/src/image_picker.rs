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

struct Queue {
    buffer: Vec<PathBuf>,
    current: usize,
    tail: usize,
    size: usize,
}

impl Queue {
    fn with_capacity(size: usize) -> Self {
        Self {
            buffer: Vec::with_capacity(size),
            current: 0,
            tail: size - 1,
            size,
        }
    }

    #[cfg(test)]
    fn current(&self) -> &Path {
        &self.buffer[self.current]
    }

    fn next(&mut self) -> Option<&Path> {
        let next_index = (self.current + 1) % self.size;
        if !self.is_full() {
            if next_index < self.buffer.len() {
                self.current = next_index;
                Some(&self.buffer[next_index])
            } else {
                None
            }
        } else if self.current != self.tail {
            self.current = next_index;
            Some(&self.buffer[next_index])
        } else {
            None
        }
    }

    fn previous(&mut self) -> Option<&Path> {
        let prev_index = (self.current + self.size - 1) % self.size;
        if prev_index != self.tail {
            self.current = prev_index;
            Some(&self.buffer[prev_index])
        } else {
            None
        }
    }

    fn is_full(&self) -> bool {
        self.buffer.len() == self.size
    }

    fn contains(&self, p: &PathBuf) -> bool {
        self.buffer.contains(p)
    }

    fn set_current_to(&mut self, p: &Path) {
        if let Some(index) = self.buffer.iter().position(|path| p == path) {
            self.current = index;
        }
    }

    fn push(&mut self, p: PathBuf) {
        if self.is_full() {
            if self.current == self.tail {
                let next_index = (self.current + 1) % self.size;
                self.buffer[next_index] = p;
                self.current = next_index;
                self.tail = next_index;
            }
        } else {
            self.buffer.push(p);
            self.current = self.buffer.len() - 1;
        }
    }

    fn has_reached_end(&self) -> bool {
        self.current == self.tail
    }

    fn resize(&mut self, new_size: usize) {
        if !self.is_full() {
            self.buffer.reserve_exact(new_size);
            self.size = new_size;
        } else {
            let relative_current = (self.current + self.size - self.tail) % self.size;
            self.current = (self.tail + self.size - new_size % self.size) % self.size;
            let relative_current = (relative_current + self.size - self.current - 1) % new_size;
            let mut new_buf = Vec::new();
            while let Some(prev) = self.next() {
                new_buf.push(prev.to_path_buf());
            }
            self.current = relative_current;
            self.tail = new_size - 1;
            self.size = new_size;
            self.buffer = new_buf;
        }
    }
}

enum ImagePickerAction {
    Next,
    Previous,
}

enum ImagePickerSorting {
    Random(Queue),
    Ascending(usize),
    Descending(usize),
}

impl ImagePickerSorting {
    fn new_random(queue_size: usize) -> Self {
        ImagePickerSorting::Random(Queue::with_capacity(queue_size))
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
    pub const DEFAULT_DRAWN_IMAGES_QUEUE_SIZE: usize = 10;
    pub fn new(wallpaper_info: &WallpaperInfo, filelist_cache: Rc<RefCell<FilelistCache>>) -> Self {
        Self {
            current_img: PathBuf::from(""),
            image_changed_instant: Instant::now(),
            action: Some(ImagePickerAction::Next),
            sorting: match wallpaper_info.sorting {
                Sorting::Random => {
                    ImagePickerSorting::new_random(wallpaper_info.drawn_images_queue_size)
                }
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
            (None | Some(ImagePickerAction::Next), ImagePickerSorting::Random(queue)) => {
                // Use the next images in the queue, if any
                while let Some(next) = queue.next() {
                    if next.exists() {
                        return (usize::MAX, next.to_path_buf());
                    }
                }
                let mut tries = 5;
                // Otherwise pick a new random image
                loop {
                    let index = rand::random::<usize>() % files.len();
                    // search for an image that has not been drawn yet
                    // fail after 5 tries
                    if tries == 0 || !queue.contains(&files[index]) {
                        break (index, files[index].to_path_buf());
                    }

                    tries -= 1;
                }
            }
            (Some(ImagePickerAction::Previous), ImagePickerSorting::Random(queue)) => {
                while let Some(prev) = queue.previous() {
                    if prev.exists() {
                        return (usize::MAX, prev.to_path_buf());
                    }
                }

                // We didn't find any suitable image, reset to the last working one
                queue.set_current_to(&self.current_img.to_path_buf());
                (usize::MAX, self.current_image())
            }
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
                            (Some(ImagePickerAction::Next), ImagePickerSorting::Random(queue))
                                if queue.has_reached_end() =>
                            {
                                queue.push(img_path.clone());
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
    pub fn update_sorting(
        &mut self,
        new_sorting: Sorting,
        path_changed: bool,
        drawn_images_queue_size: usize,
    ) {
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
                self.sorting = ImagePickerSorting::new_random(drawn_images_queue_size);
            }
            (
                ImagePickerSorting::Descending(_) | ImagePickerSorting::Ascending(_),
                Sorting::Random,
            ) => {
                // if the path was not changed, use the current image as the first image of
                // the drawn_images
                let mut queue = Queue::with_capacity(drawn_images_queue_size);
                queue.push(self.current_image());
                self.sorting = ImagePickerSorting::Random(queue);
            }
            // The path has changed, use a new random sorting, otherwise we reuse the current
            // drawn_images
            (ImagePickerSorting::Random { .. }, Sorting::Random) if path_changed => {
                self.sorting = ImagePickerSorting::new_random(drawn_images_queue_size);
            }
            // No need to update the sorting if it's the same
            (_, _) => {}
        }
    }

    pub fn update_queue_size(&mut self, drawn_images_queue_size: usize) {
        match &mut self.sorting {
            ImagePickerSorting::Random(queue) => {
                queue.resize(drawn_images_queue_size);
            }
            ImagePickerSorting::Ascending(_) | ImagePickerSorting::Descending(_) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    // Note this useful idiom: importing names from outer (for mod tests) scope.
    use super::*;

    #[test]
    fn test_push() {
        let mut queue = Queue::with_capacity(2);
        queue.push(PathBuf::from("mypath"));
        queue.push(PathBuf::from("mypath2"));
        assert_eq!(Path::new("mypath2"), queue.current());
        assert_eq!(Some(Path::new("mypath")), queue.previous());
        assert_eq!(Path::new("mypath"), queue.current());

        assert_eq!(None, queue.previous());

        // Check that the buffer is circular
        queue.next();
        queue.push(PathBuf::from("mypath3"));
        assert_eq!(Path::new("mypath3"), queue.current());
        assert_eq!(Some(Path::new("mypath2")), queue.previous());
        assert_eq!(None, queue.previous());
    }

    #[test]
    fn test_resize() {
        let mut queue = Queue::with_capacity(5);
        queue.push(PathBuf::from("mypath"));
        queue.push(PathBuf::from("mypath2"));
        queue.push(PathBuf::from("mypath3"));
        queue.push(PathBuf::from("mypath4"));
        queue.push(PathBuf::from("mypath5"));
        assert_eq!(Path::new("mypath5"), queue.current());
        assert_eq!(Some(Path::new("mypath4")), queue.previous());

        // Test that the current index works when it's inside the resizing range
        queue.resize(2);
        assert_eq!(Path::new("mypath4"), queue.current());
        assert_eq!(None, queue.previous());
        assert_eq!(Some(Path::new("mypath5")), queue.next());
    }

    #[test]
    fn test_resize2() {
        let mut queue = Queue::with_capacity(5);
        queue.push(PathBuf::from("mypath"));
        queue.push(PathBuf::from("mypath2"));
        queue.push(PathBuf::from("mypath3"));
        queue.push(PathBuf::from("mypath4"));
        queue.push(PathBuf::from("mypath5"));
        queue.push(PathBuf::from("mypath6"));
        queue.push(PathBuf::from("mypath7"));
        queue.push(PathBuf::from("mypath8"));
        assert_eq!(Path::new("mypath8"), queue.current());
        assert_eq!(Some(Path::new("mypath7")), queue.previous());
        assert_eq!(Some(Path::new("mypath6")), queue.previous());
        assert_eq!(Some(Path::new("mypath5")), queue.previous());

        // Test that the current item point to the first item available
        queue.resize(2);
        assert_eq!(Path::new("mypath7"), queue.current());
        assert_eq!(Some(Path::new("mypath8")), queue.next());
        assert_eq!(None, queue.next());
    }
}
