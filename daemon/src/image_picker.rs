use std::{
    cell::RefCell,
    collections::VecDeque,
    path::{Path, PathBuf},
    rc::Rc,
    time::Instant,
};

use log::warn;

use crate::{
    filelist_cache::FilelistCache,
    wallpaper_info::{Sorting, WallpaperInfo},
};

#[derive(Debug)]
struct Queue {
    buffer: VecDeque<PathBuf>,
    current: usize,
    tail: usize,
    size: usize,
}

impl Queue {
    fn with_capacity(size: usize) -> Self {
        Self {
            buffer: VecDeque::with_capacity(size),
            current: 0,
            tail: size - 1,
            size,
        }
    }

    #[cfg(test)]
    fn current(&self) -> &Path {
        &self.buffer[self.current]
    }

    fn next(&mut self) -> Option<(&Path, usize)> {
        let next_index = (self.current + 1) % self.size;
        if !self.is_full() {
            if next_index < self.buffer.len() {
                self.current = next_index;
                Some((&self.buffer[next_index], next_index))
            } else {
                None
            }
        } else if self.current != self.tail {
            self.current = next_index;
            Some((&self.buffer[next_index], next_index))
        } else {
            None
        }
    }

    fn previous(&mut self) -> Option<(&Path, usize)> {
        let prev_index = (self.current + self.size - 1) % self.size;
        if prev_index != self.tail {
            self.current = prev_index;
            Some((&self.buffer[prev_index], prev_index))
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
        // Avoid duplicates
        if self.buffer.contains(&p) {
            return;
        };

        if self.is_full() {
            self.buffer.pop_front();
            self.buffer.push_back(p);
        } else {
            self.buffer.push_back(p);
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
            let mut new_buf = VecDeque::new();
            while let Some((prev, _)) = self.next() {
                new_buf.push_back(prev.to_path_buf());
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
    reload: bool,
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
                Sorting::Ascending => ImagePickerSorting::Ascending(
                    filelist_cache.borrow().get(&wallpaper_info.path).len() - 1,
                ),
                Sorting::Descending => ImagePickerSorting::Descending(0),
            },
            filelist_cache,
            reload: false,
        }
    }

    /// Get the next image based on the sorting method
    fn get_image_path(&mut self, files: &[PathBuf]) -> (usize, PathBuf) {
        match (&self.action, &mut self.sorting) {
            (
                None,
                ImagePickerSorting::Ascending(current_index)
                | ImagePickerSorting::Descending(current_index),
            ) if self.current_img.exists() => (*current_index, self.current_img.to_path_buf()),

            (None, ImagePickerSorting::Random(_)) if self.current_img.exists() => {
                (0, self.current_img.to_path_buf())
            }
            (None | Some(ImagePickerAction::Next), ImagePickerSorting::Random(queue)) => {
                // Use the next images in the queue, if any
                while let Some((next, index)) = queue.next() {
                    if next.exists() {
                        return (index, next.to_path_buf());
                    }
                }
                // If there is only one image just return it
                if files.len() == 1 {
                    return (0, files[0].to_path_buf());
                }

                // Otherwise pick a new random image that has not been drawn before
                // Try 5 times, then get a random image. We do this because it might happen
                // that the queue is bigger than the amount of available wallpapers
                let mut tries = 5;
                loop {
                    let index = rand::random::<usize>() % files.len();
                    // search for an image that has not been drawn yet
                    // fail after 5 tries
                    if !queue.contains(&files[index]) {
                        break (index, files[index].to_path_buf());
                    }

                    // We have already tried a bunch of times
                    // We still need a new image, get the first one that is different than
                    // the current one. We also know that there is more than one image
                    if tries == 0 {
                        break loop {
                            let index = rand::random::<usize>() % files.len();
                            if files[index] != self.current_img {
                                break (index, files[index].to_path_buf());
                            }
                        };
                    }

                    tries -= 1;
                }
            }
            (Some(ImagePickerAction::Previous), ImagePickerSorting::Random(queue)) => {
                while let Some((prev, index)) = queue.previous() {
                    if prev.exists() {
                        return (index, prev.to_path_buf());
                    }
                }

                // We didn't find any suitable image, reset to the last working one
                queue.set_current_to(&self.current_img.to_path_buf());
                (usize::MAX, self.current_image())
            }
            (
                None | Some(ImagePickerAction::Next),
                ImagePickerSorting::Descending(current_index),
            )
            | (Some(ImagePickerAction::Previous), ImagePickerSorting::Ascending(current_index)) => {
                let index = if files.get(*current_index) == Some(&self.current_img) {
                    *current_index
                } else {
                    // if the current img doesn't correspond to the index we have
                    // try looking for it in files
                    match files.binary_search(&self.current_img) {
                        Ok(new_index) => new_index,
                        Err(_err) => {
                            // if we don't find it, use the last index as starting point
                            // if the current_index is too big, start from last image
                            // this is a fail safe in case many files gets deleted
                            if *current_index >= files.len() {
                                0
                            } else {
                                *current_index
                            }
                        }
                    }
                };
                let index = if index == 0 {
                    files.len() - 1
                } else {
                    index - 1
                };
                (index, files[index].to_path_buf())
            }
            (Some(ImagePickerAction::Previous), ImagePickerSorting::Descending(current_index))
            | (
                None | Some(ImagePickerAction::Next),
                ImagePickerSorting::Ascending(current_index),
            ) => {
                let index = if files.get(*current_index) == Some(&self.current_img) {
                    *current_index
                } else {
                    match files.binary_search(&self.current_img) {
                        Ok(new_index) => new_index,
                        Err(_err) => *current_index,
                    }
                };
                let index = (index + 1) % files.len();
                (index, files[index].to_path_buf())
            }
        }
    }

    pub fn get_image_from_path(&mut self, path: &Path) -> Option<(PathBuf, usize)> {
        if path.is_dir() {
            let files = self.filelist_cache.borrow().get(path);

            // There are no images, forcefully break out of the loop
            if files.is_empty() {
                warn!("Directory {path:?} does not contain any valid image files.");
                None
            } else {
                let (index, img_path) = self.get_image_path(&files);
                if img_path == self.current_img && !self.reload {
                    None
                } else {
                    Some((img_path, index))
                }
            }
        } else if path == self.current_img && !self.reload {
            None
        } else {
            // path is not a directory, also it's not the current image or we need to reload
            Some((path.to_path_buf(), 0))
        }
    }

    pub fn update_current_image(&mut self, img_path: PathBuf, index: usize) {
        match (self.action.take(), &mut self.sorting) {
            (Some(ImagePickerAction::Next), ImagePickerSorting::Random(queue)) => {
                if queue.has_reached_end() || queue.buffer.get(index).is_none() {
                    queue.push(img_path.clone());
                }
            }
            (None | Some(ImagePickerAction::Previous), ImagePickerSorting::Random { .. }) => {}
            (
                _,
                ImagePickerSorting::Ascending(current_index)
                | ImagePickerSorting::Descending(current_index),
            ) => *current_index = index,
        }

        self.current_img = img_path;
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
            // The path has changed, use a new random sorting, otherwise we reuse the current
            // drawn_images
            (ImagePickerSorting::Random { .. }, Sorting::Random) if path_changed => {
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

    #[inline]
    pub fn reload(&mut self) {
        self.reload = true;
    }

    #[inline]
    pub fn reloaded(&mut self) {
        self.reload = false;
    }

    #[inline]
    pub fn is_reloading(&self) -> bool {
        self.reload
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
        assert_eq!(Some((Path::new("mypath"), 0)), queue.previous());
        assert_eq!(Path::new("mypath"), queue.current());

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
        assert_eq!(queue.buffer.len(), 5);
        assert_eq!(Path::new("mypath5"), queue.current());
        assert_eq!(Some((Path::new("mypath4"), 3)), queue.previous());

        // Test that the current index works when it's inside the resizing range
        queue.resize(2);
        assert_eq!(Path::new("mypath4"), queue.current());
        assert_eq!(None, queue.previous());
        assert_eq!(Some((Path::new("mypath5"), 1)), queue.next());
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
        assert_eq!(queue.buffer.len(), 5);
        assert_eq!(Path::new("mypath8"), queue.current());
        assert_eq!(Some((Path::new("mypath7"), 3)), queue.previous());
        assert_eq!(Some((Path::new("mypath6"), 2)), queue.previous());
        assert_eq!(Some((Path::new("mypath5"), 1)), queue.previous());

        // Test that the current item point to the first item available
        queue.resize(2);
        assert_eq!(Path::new("mypath7"), queue.current());
        assert_eq!(Some((Path::new("mypath8"), 1)), queue.next());
        assert_eq!(None, queue.next());
    }
}
