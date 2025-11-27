use std::{
    cell::RefCell,
    collections::VecDeque,
    path::{Path, PathBuf},
    rc::Rc,
};

use log::warn;
use smithay_client_toolkit::reexports::client::{protocol::wl_surface::WlSurface, QueueHandle};

use crate::{
    filelist_cache::FilelistCache,
    wallpaper_groups::{WallpaperGroup, WallpaperGroups},
    wallpaper_info::{Recursive, Sorting, WallpaperInfo},
    wpaperd::Wpaperd,
};

/// Result from `get_image_from_path` - distinguishes forced images from list-based images.
/// This makes the contract explicit: forced images have no meaningful index.
#[derive(Debug, Clone)]
pub enum ImageResult {
    /// Image was forced via `wpaperctl set` - not part of normal navigation
    Forced(PathBuf),
    /// Image from the configured directory/list with its index
    FromList { path: PathBuf, index: usize },
}

impl ImageResult {
    pub fn path(&self) -> &Path {
        match self {
            ImageResult::Forced(p) => p,
            ImageResult::FromList { path, .. } => path,
        }
    }
}

#[derive(Debug)]
pub struct Queue {
    buffer: VecDeque<PathBuf>,
    current: usize,
    tail: usize,
    size: usize,
}

impl Queue {
    pub fn with_capacity(size: usize) -> Self {
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

struct GroupedRandom {
    surface: WlSurface,
    group: Rc<RefCell<WallpaperGroup>>,
    groups: Rc<RefCell<WallpaperGroups>>,
}

impl GroupedRandom {
    fn new(
        groups: Rc<RefCell<WallpaperGroups>>,
        group: u8,
        wl_surface: &WlSurface,
        queue_size: usize,
    ) -> Self {
        Self {
            surface: wl_surface.clone(),
            group: groups
                .borrow_mut()
                .get_or_insert(group, wl_surface, queue_size),
            groups: groups.clone(),
        }
    }
}

impl Drop for GroupedRandom {
    fn drop(&mut self) {
        let group = self.group.borrow();
        let group_index = group.group;
        drop(group);
        self.groups.borrow_mut().remove(group_index, &self.surface);
    }
}

enum ImagePickerSorting {
    Random(Queue),
    GroupedRandom(GroupedRandom),
    Ascending(usize),
    Descending(usize),
}

impl ImagePickerSorting {
    fn new(
        wallpaper_info: &WallpaperInfo,
        wl_surface: &WlSurface,
        groups: Rc<RefCell<WallpaperGroups>>,
        filelist_cache: Rc<RefCell<FilelistCache>>,
    ) -> Self {
        match wallpaper_info.sorting {
            None | Some(Sorting::Random) => {
                Self::new_random(wallpaper_info.drawn_images_queue_size)
            }
            Some(Sorting::GroupedRandom { group }) => {
                ImagePickerSorting::GroupedRandom(GroupedRandom::new(
                    groups,
                    group,
                    wl_surface,
                    wallpaper_info.drawn_images_queue_size,
                ))
            }
            Some(Sorting::Ascending) => {
                let files_len = filelist_cache
                    .clone()
                    .borrow()
                    .get(
                        &wallpaper_info.path,
                        wallpaper_info.recursive.unwrap_or_default(),
                    )
                    .len();
                Self::new_ascending(files_len)
            }
            Some(Sorting::Descending) => Self::new_descending(),
        }
    }

    fn new_random(queue_size: usize) -> Self {
        Self::Random(Queue::with_capacity(queue_size))
    }

    fn new_descending() -> ImagePickerSorting {
        Self::Descending(0)
    }

    fn new_ascending(files_len: usize) -> ImagePickerSorting {
        Self::Ascending(files_len - 1)
    }
}

pub struct ImagePicker {
    current_img: PathBuf,
    action: Option<ImagePickerAction>,
    sorting: ImagePickerSorting,
    filelist_cache: Rc<RefCell<FilelistCache>>,
    reload: bool,
    /// Forced image path from `wpaperctl set`, consumed on next get_image_from_path call
    forced_image: Option<PathBuf>,
    /// True if the currently displayed image was set via `wpaperctl set`
    was_last_forced: bool,
}

impl ImagePicker {
    pub const DEFAULT_DRAWN_IMAGES_QUEUE_SIZE: usize = 10;
    pub fn new(
        wallpaper_info: &WallpaperInfo,
        wl_surface: &WlSurface,
        filelist_cache: Rc<RefCell<FilelistCache>>,
        groups: Rc<RefCell<WallpaperGroups>>,
    ) -> Self {
        Self {
            current_img: PathBuf::from(""),
            action: Some(ImagePickerAction::Next),
            sorting: ImagePickerSorting::new(
                wallpaper_info,
                wl_surface,
                groups,
                filelist_cache.clone(),
            ),
            filelist_cache,
            reload: false,
            forced_image: None,
            was_last_forced: false,
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
            (_, ImagePickerSorting::GroupedRandom(group))
                if group.group.borrow().loading_image.is_some() =>
            {
                let group = group.group.borrow();
                let (index, loading_image) = group.loading_image.as_ref().unwrap();
                (*index, loading_image.to_path_buf())
            }
            (_, ImagePickerSorting::GroupedRandom(group))
                if group.group.borrow().current_image != self.current_img =>
            {
                let group = group.group.borrow();
                (group.index, group.current_image.clone())
            }
            (None, ImagePickerSorting::Random(_) | ImagePickerSorting::GroupedRandom(_))
                if self.current_img.exists() =>
            {
                (0, self.current_img.to_path_buf())
            }
            (None | Some(ImagePickerAction::Next), ImagePickerSorting::Random(queue)) => {
                next_random_image(&self.current_img, queue, files)
            }
            (None | Some(ImagePickerAction::Next), ImagePickerSorting::GroupedRandom(group)) => {
                let mut group = group.group.borrow_mut();
                if self.current_img == group.current_image {
                    // start loading a new image
                    let (index, path) =
                        next_random_image(&self.current_img, &mut group.queue, files);
                    group.loading_image = Some((index, path.to_path_buf()));
                    (index, path)
                } else {
                    (group.index, group.current_image.clone())
                }
            }
            (Some(ImagePickerAction::Previous), ImagePickerSorting::Random(queue)) => {
                get_previous_image_for_random(&self.current_img, queue)
            }
            (Some(ImagePickerAction::Previous), ImagePickerSorting::GroupedRandom(group)) => {
                let mut group = group.group.borrow_mut();
                let queue = &mut group.queue;
                let (index, path) = get_previous_image_for_random(&self.current_img, queue);
                if path != group.current_image {
                    group.loading_image = Some((index, path.to_path_buf()));
                }
                (index, path)
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

    /// Set the wallpaper to a specific image path.
    /// The forced image will be returned by the next call to `get_image_from_path`.
    pub fn set_image(&mut self, path: PathBuf) {
        self.forced_image = Some(path);
    }

    pub fn get_image_from_path(
        &mut self,
        path: &Path,
        recursive: &Option<Recursive>,
    ) -> Option<ImageResult> {
        // Check for forced image first (from wpaperctl set)
        // Don't update navigation state - forced images are "detours"
        if let Some(forced_path) = self.forced_image.take() {
            self.was_last_forced = true;
            return Some(ImageResult::Forced(forced_path));
        }

        // Clear flag for normal image loads
        self.was_last_forced = false;

        if path.is_dir() {
            let files = self
                .filelist_cache
                .borrow()
                .get(path, recursive.unwrap_or_default());

            // There are no images, forcefully break out of the loop
            if files.is_empty() {
                warn!("Directory {path:?} does not contain any valid image files.");
                None
            } else {
                let (index, img_path) = self.get_image_path(&files);
                if img_path == self.current_img && !self.reload {
                    None
                } else {
                    Some(ImageResult::FromList {
                        path: img_path,
                        index,
                    })
                }
            }
        } else if path == self.current_img && !self.reload {
            None
        } else {
            // path is not a directory, also it's not the current image or we need to reload
            Some(ImageResult::FromList {
                path: path.to_path_buf(),
                index: 0,
            })
        }
    }

    pub fn update_current_image(&mut self, result: ImageResult) {
        match result {
            ImageResult::Forced(img_path) => {
                // Don't update navigation state for forced images - they're "detours"
                // Clear action without updating state
                self.action.take();
                self.current_img = img_path;
            }
            ImageResult::FromList {
                path: img_path,
                index,
            } => {
                match (self.action.take(), &mut self.sorting) {
                    (Some(ImagePickerAction::Next), ImagePickerSorting::Random(queue)) => {
                        queue.push(img_path.clone());
                    }
                    (
                        None | Some(ImagePickerAction::Previous),
                        ImagePickerSorting::Random { .. },
                    ) => {}
                    (
                        None | Some(ImagePickerAction::Previous),
                        ImagePickerSorting::GroupedRandom(group),
                    ) => {
                        let mut group = group.group.borrow_mut();
                        group.loading_image = None;
                        group.current_image.clone_from(&img_path);
                        group.index = index;
                    }
                    (
                        _,
                        ImagePickerSorting::Ascending(current_index)
                        | ImagePickerSorting::Descending(current_index),
                    ) => *current_index = index,
                    (Some(ImagePickerAction::Next), ImagePickerSorting::GroupedRandom(group)) => {
                        let mut group = group.group.borrow_mut();
                        let queue = &mut group.queue;
                        queue.push(img_path.clone());
                        group.loading_image = None;
                        group.current_image.clone_from(&img_path);
                        group.index = index;
                    }
                }
                self.current_img = img_path;
            }
        }
    }

    /// Update wallpaper by going down 1 index through the cached image paths.
    /// Expiry timer reset even if already at the first cached image.
    ///
    /// # Detour Semantics
    ///
    /// If the current image was set via `wpaperctl set`, this returns to the
    /// image that was showing before the set (doesn't decrement the index).
    /// This implements "detour" behavior: forced images are temporary diversions
    /// that don't affect navigation history.
    ///
    /// # Implementation Note
    ///
    /// When returning from a forced image, we reuse the `reload` flag to trigger
    /// a redisplay of the current index position. This is a non-standard use of
    /// `reload` (which normally means "refresh current image from disk"), but it
    /// achieves the desired behavior without adding another flag. The `reload`
    /// flag causes `get_image_from_path` to return the current index's image
    /// even if it matches `current_img`.
    pub fn previous_image(&mut self) {
        if self.was_last_forced {
            // DETOUR RETURN: User called `previous` after a forced image.
            // Instead of going back in history, return to the image that was
            // showing before the forced image. We reuse `reload` to trigger
            // `get_image_from_path` to re-fetch the current index position.
            self.reload = true;
            self.was_last_forced = false;
        } else {
            self.action = Some(ImagePickerAction::Previous);
        }
    }

    /// Update wallpaper by going up 1 index through the cached image paths
    pub fn next_image(&mut self, path: &Path, recursive: &Option<Recursive>) {
        // Clear forced flag - next continues normal navigation
        self.was_last_forced = false;
        self.action = Some(ImagePickerAction::Next);
        self.get_image_from_path(path, recursive);
    }

    pub fn current_image(&self) -> PathBuf {
        self.current_img.clone()
    }

    /// Return true if the path changed
    pub fn update_sorting(
        &mut self,
        wallpaper_info: &WallpaperInfo,
        wl_surface: &WlSurface,
        path_changed: bool,
        wallpaper_groups: &Rc<RefCell<WallpaperGroups>>,
    ) {
        if let Some(new_sorting) = wallpaper_info.sorting {
            match (&mut self.sorting, new_sorting) {
                // If the the sorting stayed the same, do nothing
                (ImagePickerSorting::Ascending(_), Sorting::Ascending)
                | (ImagePickerSorting::Descending(_), Sorting::Descending)
                | (ImagePickerSorting::Random(_), Sorting::Random)
                    if !path_changed => {}
                (_, Sorting::Ascending) if path_changed => {
                    self.sorting = ImagePickerSorting::new_ascending(
                        self.filelist_cache
                            .borrow()
                            .get(
                                &wallpaper_info.path,
                                wallpaper_info.recursive.unwrap_or_default(),
                            )
                            .len(),
                    );
                }
                (_, Sorting::Descending) if path_changed => {
                    self.sorting = ImagePickerSorting::new_descending();
                }
                (_, Sorting::Ascending | Sorting::Descending) => {
                    let index = self.get_current_index();
                    self.sorting = match new_sorting {
                        Sorting::Random | Sorting::GroupedRandom { .. } => unreachable!(),
                        Sorting::Ascending => ImagePickerSorting::Ascending(index),
                        Sorting::Descending => ImagePickerSorting::Descending(index),
                    };
                }
                // The path has changed, use a new random sorting, otherwise we reuse the current
                // drawn_images
                (_, Sorting::Random) if path_changed => {
                    self.sorting =
                        ImagePickerSorting::new_random(wallpaper_info.drawn_images_queue_size);
                }
                (_, Sorting::Random) => {
                    // if the path was not changed, use the current image as the first image of
                    // the drawn_images
                    let mut queue = Queue::with_capacity(wallpaper_info.drawn_images_queue_size);
                    queue.push(self.current_image());
                    self.sorting = ImagePickerSorting::Random(queue);
                }
                (_, Sorting::GroupedRandom { group }) if path_changed => {
                    self.sorting = ImagePickerSorting::GroupedRandom(GroupedRandom::new(
                        wallpaper_groups.clone(),
                        group,
                        wl_surface,
                        wallpaper_info.drawn_images_queue_size,
                    ));
                }
                // If the group is the same
                (
                    ImagePickerSorting::GroupedRandom(grouped_random),
                    Sorting::GroupedRandom { group },
                ) if grouped_random.group.borrow().group == group => {}
                (_, Sorting::GroupedRandom { group }) => {
                    let grouped_random = GroupedRandom::new(
                        wallpaper_groups.clone(),
                        group,
                        wl_surface,
                        wallpaper_info.drawn_images_queue_size,
                    );

                    let mut group = grouped_random.group.borrow_mut();
                    // If there are no other surfaces, we must reuse the current wallpaper
                    if group.surfaces.len() == 1 {
                        group.current_image = self.current_img.clone();
                        group.index = self.get_current_index();
                        group.queue.push(self.current_img.clone());
                    }

                    drop(group);
                    self.sorting = ImagePickerSorting::GroupedRandom(grouped_random);
                }
            }
        } else {
            self.sorting = ImagePickerSorting::new_random(wallpaper_info.drawn_images_queue_size);
        }
    }

    fn get_current_index(&mut self) -> usize {
        match &self.sorting {
            ImagePickerSorting::Random(queue) => queue.current,
            // This is already covered above
            ImagePickerSorting::GroupedRandom(old_grouped_random) => {
                old_grouped_random.group.borrow().index
            }
            ImagePickerSorting::Ascending(index) | ImagePickerSorting::Descending(index) => *index,
        }
    }

    pub fn update_queue_size(&mut self, drawn_images_queue_size: usize) {
        match &mut self.sorting {
            ImagePickerSorting::Random(queue) => {
                queue.resize(drawn_images_queue_size);
            }
            ImagePickerSorting::Ascending(_) | ImagePickerSorting::Descending(_) => {}
            ImagePickerSorting::GroupedRandom(group) => {
                group
                    .group
                    .borrow_mut()
                    .queue
                    .resize(drawn_images_queue_size);
            }
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

    pub fn handle_grouped_sorting(&self, qh: &QueueHandle<Wpaperd>) {
        if let ImagePickerSorting::GroupedRandom(grouped_random) = &self.sorting {
            grouped_random.group.borrow().queue_all_surfaces(qh);
        }
    }
}

fn next_random_image(
    current_image: &Path,
    queue: &mut Queue,
    files: &[PathBuf],
) -> (usize, PathBuf) {
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
        let index = fastrand::usize(..files.len());
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
                let index = fastrand::usize(..files.len());
                if files[index] != current_image {
                    break (index, files[index].to_path_buf());
                }
            };
        }

        tries -= 1;
    }
}

fn get_previous_image_for_random(current_image: &Path, queue: &mut Queue) -> (usize, PathBuf) {
    while let Some((prev, index)) = queue.previous() {
        if prev.exists() {
            return (index, prev.to_path_buf());
        }
    }

    // We didn't find any suitable image, reset to the last working one
    queue.set_current_to(current_image);
    (usize::MAX, current_image.to_path_buf())
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

    // =======================================================
    // Tests for Queue interaction with "set" behavior
    // =======================================================

    #[test]
    fn test_queue_set_then_previous_navigates_back() {
        let mut queue = Queue::with_capacity(5);

        // Build up some history
        queue.push(PathBuf::from("/image1.png"));
        queue.push(PathBuf::from("/image2.png"));
        queue.push(PathBuf::from("/image3.png"));

        // User sets a manual wallpaper (push simulates adding to history)
        queue.push(PathBuf::from("/manual.png"));
        assert_eq!(queue.current(), Path::new("/manual.png"));

        // User immediately does `previous` - should go to image3
        let prev = queue.previous();
        assert_eq!(prev, Some((Path::new("/image3.png"), 2)));
    }

    #[test]
    fn test_queue_set_existing_image_is_noop() {
        let mut queue = Queue::with_capacity(5);

        queue.push(PathBuf::from("/image1.png"));
        queue.push(PathBuf::from("/image2.png"));

        // User sets the same image that's already current
        // Queue::push already handles this - duplicates are ignored
        queue.push(PathBuf::from("/image2.png"));

        // Still at image2, buffer should not have grown
        assert_eq!(queue.current(), Path::new("/image2.png"));
        assert_eq!(queue.buffer.len(), 2);

        // Previous goes to image1
        let prev = queue.previous();
        assert_eq!(prev, Some((Path::new("/image1.png"), 0)));

        // No more previous (we're at the start)
        assert_eq!(queue.previous(), None);
    }

    #[test]
    fn test_queue_set_current_to_moves_cursor() {
        let mut queue = Queue::with_capacity(5);

        queue.push(PathBuf::from("/image1.png"));
        queue.push(PathBuf::from("/image2.png"));
        queue.push(PathBuf::from("/image3.png"));
        assert_eq!(queue.current(), Path::new("/image3.png"));

        // set_current_to moves cursor to existing image in history
        queue.set_current_to(Path::new("/image1.png"));
        assert_eq!(queue.current(), Path::new("/image1.png"));

        // Next should go to image2
        let next = queue.next();
        assert_eq!(next, Some((Path::new("/image2.png"), 1)));
    }

    #[test]
    fn test_queue_set_current_to_nonexistent_is_noop() {
        let mut queue = Queue::with_capacity(5);

        queue.push(PathBuf::from("/image1.png"));
        queue.push(PathBuf::from("/image2.png"));
        assert_eq!(queue.current(), Path::new("/image2.png"));

        // Try to set to a path not in history - set_current_to is a no-op
        queue.set_current_to(Path::new("/nonexistent.png"));

        // Current should still be image2
        assert_eq!(queue.current(), Path::new("/image2.png"));
    }

    // =======================================================
    // Tests for ImageResult enum
    // =======================================================

    #[test]
    fn test_image_result_path_returns_correct_path() {
        let forced = ImageResult::Forced(PathBuf::from("/forced.png"));
        assert_eq!(forced.path(), Path::new("/forced.png"));

        let from_list = ImageResult::FromList {
            path: PathBuf::from("/list.png"),
            index: 42,
        };
        assert_eq!(from_list.path(), Path::new("/list.png"));
    }

    #[test]
    fn test_image_result_forced_variant() {
        let result = ImageResult::Forced(PathBuf::from("/test.png"));
        assert!(matches!(result, ImageResult::Forced(_)));
    }

    #[test]
    fn test_image_result_from_list_variant() {
        let result = ImageResult::FromList {
            path: PathBuf::from("/test.png"),
            index: 5,
        };
        match result {
            ImageResult::FromList { path, index } => {
                assert_eq!(path, PathBuf::from("/test.png"));
                assert_eq!(index, 5);
            }
            _ => panic!("Expected FromList variant"),
        }
    }

    // =======================================================
    // Tests for ImagePicker forced image state transitions
    // =======================================================
    //
    // Note: ImagePicker requires complex dependencies (WlSurface, FilelistCache, etc.)
    // that are difficult to construct in unit tests. These tests focus on the
    // state machine logic by testing the individual state transitions.
    //
    // Integration testing of the full set->previous/next flow should be done
    // via manual testing with the actual daemon.

    /// Helper struct to test ImagePicker state transitions without full dependencies
    struct ForcedImageState {
        was_last_forced: bool,
        reload: bool,
        action: Option<ImagePickerAction>,
    }

    impl ForcedImageState {
        fn new() -> Self {
            Self {
                was_last_forced: false,
                reload: false,
                action: None,
            }
        }

        /// Simulates the state change when a forced image is loaded
        fn simulate_forced_image_loaded(&mut self) {
            self.was_last_forced = true;
        }

        /// Simulates previous_image() behavior
        fn previous_image(&mut self) {
            if self.was_last_forced {
                // DETOUR RETURN: reload current index instead of going back
                self.reload = true;
                self.was_last_forced = false;
            } else {
                self.action = Some(ImagePickerAction::Previous);
            }
        }

        /// Simulates next_image() behavior (just the state transition part)
        fn next_image(&mut self) {
            self.was_last_forced = false;
            self.action = Some(ImagePickerAction::Next);
        }
    }

    #[test]
    fn test_forced_image_previous_triggers_reload() {
        let mut state = ForcedImageState::new();

        // Simulate: user did `wpaperctl set`, image loaded
        state.simulate_forced_image_loaded();
        assert!(state.was_last_forced);
        assert!(!state.reload);

        // User calls `previous`
        state.previous_image();

        // Should trigger reload (return to pre-set image), NOT set Previous action
        assert!(state.reload);
        assert!(!state.was_last_forced);
        assert!(state.action.is_none());
    }

    #[test]
    fn test_forced_image_next_clears_flag_and_continues() {
        let mut state = ForcedImageState::new();

        // Simulate: user did `wpaperctl set`, image loaded
        state.simulate_forced_image_loaded();
        assert!(state.was_last_forced);

        // User calls `next`
        state.next_image();

        // Should clear forced flag and continue normal navigation
        assert!(!state.was_last_forced);
        assert!(matches!(state.action, Some(ImagePickerAction::Next)));
    }

    #[test]
    fn test_normal_previous_sets_action() {
        let mut state = ForcedImageState::new();

        // No forced image - normal state
        assert!(!state.was_last_forced);

        // User calls `previous`
        state.previous_image();

        // Should set Previous action, not reload
        assert!(!state.reload);
        assert!(matches!(state.action, Some(ImagePickerAction::Previous)));
    }

    #[test]
    fn test_detour_semantics_sequence() {
        let mut state = ForcedImageState::new();

        // 1. User is viewing normal image, calls next a few times
        state.next_image();
        assert!(matches!(state.action, Some(ImagePickerAction::Next)));
        state.action = None; // Reset for next operation

        // 2. User does `wpaperctl set /some/image.png`
        state.simulate_forced_image_loaded();
        assert!(state.was_last_forced);

        // 3. User calls `previous` - should return to pre-set image (reload)
        state.previous_image();
        assert!(state.reload);
        assert!(!state.was_last_forced);
        assert!(state.action.is_none()); // No Previous action - reload handles it

        // 4. Reset reload flag (simulating it was consumed)
        state.reload = false;

        // 5. Now user calls `previous` again - normal navigation
        state.previous_image();
        assert!(!state.reload);
        assert!(matches!(state.action, Some(ImagePickerAction::Previous)));
    }
}
