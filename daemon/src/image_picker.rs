use std::{
    cell::RefCell,
    collections::{HashSet, VecDeque},
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

#[derive(Debug)]
pub struct Queue {
    buffer: VecDeque<PathBuf>,
    current: usize,
    tail: usize,
    size: usize,
    // How many of the most recent entries belong to the current cycle,
    // i.e. have been shown since the last time every available image
    // had been seen. Random selection avoids these.
    in_cycle: usize,
}

impl Queue {
    pub fn with_capacity(size: usize) -> Self {
        Self {
            buffer: VecDeque::with_capacity(size),
            current: 0,
            tail: size - 1,
            size,
            in_cycle: 0,
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

    fn current_cycle(&self) -> impl Iterator<Item = &PathBuf> {
        self.buffer.iter().skip(self.buffer.len() - self.in_cycle)
    }

    fn start_new_cycle(&mut self) {
        self.in_cycle = 0;
    }

    fn set_current_to(&mut self, p: &Path) {
        if let Some(index) = self.buffer.iter().position(|path| p == path) {
            self.current = index;
        }
    }

    fn push(&mut self, p: PathBuf) {
        // Nothing to record if the cursor is already on this image: we
        // got here by replaying history, and the queue already reflects
        // what is on screen.
        if self
            .buffer
            .get(self.current)
            .map_or(false, |path| *path == p)
        {
            return;
        }

        // The image is in our history but not under the cursor, so a new
        // cycle showed it again. Move it to the most recent slot;
        // navigation should walk the order images actually appeared,
        // repeats included.
        if let Some(index) = self.buffer.iter().position(|path| *path == p) {
            // It can only have come from before the current cycle
            // started, so it joins the cycle now
            if index < self.buffer.len() - self.in_cycle {
                self.in_cycle += 1;
            }
            self.buffer.remove(index);
            self.buffer.push_back(p);
            self.current = self.buffer.len() - 1;
            return;
        }

        if self.is_full() {
            self.buffer.pop_front();
            self.buffer.push_back(p);
        } else {
            self.buffer.push_back(p);
            self.current = self.buffer.len() - 1;
        }
        self.in_cycle = (self.in_cycle + 1).min(self.buffer.len());
    }

    fn has_reached_end(&self) -> bool {
        self.current == self.tail
    }

    fn resize(&mut self, new_size: usize) {
        // A queue cannot work with zero capacity; an automatically sized
        // queue can ask for it when its folder is emptied. Keep the
        // current size until there is a real one again.
        if new_size == 0 {
            return;
        }

        // The buffer is stored oldest to newest, so shrinking means
        // dropping from the front. The cursor follows the image it was
        // on, or the oldest survivor if that image is dropped.
        while self.buffer.len() > new_size {
            self.buffer.pop_front();
            self.current = self.current.saturating_sub(1);
        }
        self.size = new_size;
        self.tail = new_size - 1;
        self.in_cycle = self.in_cycle.min(self.buffer.len());
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

/// An explicit queue-size always wins. Otherwise the queue is sized to
/// the collection itself, so that no wallpaper repeats until every one
/// has been shown. Single files and empty folders fall back to the
/// default; a queue needs room for at least one image.
fn effective_queue_size(explicit: Option<usize>, files_count: usize) -> usize {
    explicit
        .unwrap_or(if files_count == 0 {
            ImagePicker::DEFAULT_DRAWN_IMAGES_QUEUE_SIZE
        } else {
            files_count
        })
        .max(1)
}

fn queue_size_for(
    wallpaper_info: &WallpaperInfo,
    filelist_cache: &RefCell<FilelistCache>,
) -> usize {
    let files_count = if wallpaper_info.path.is_dir() {
        filelist_cache
            .borrow()
            .get(
                &wallpaper_info.path,
                wallpaper_info.recursive.unwrap_or_default(),
            )
            .len()
    } else {
        0
    };
    effective_queue_size(wallpaper_info.drawn_images_queue_size, files_count)
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
                Self::new_random(queue_size_for(wallpaper_info, &filelist_cache))
            }
            Some(Sorting::GroupedRandom { group }) => {
                ImagePickerSorting::GroupedRandom(GroupedRandom::new(
                    groups,
                    group,
                    wl_surface,
                    queue_size_for(wallpaper_info, &filelist_cache),
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

    pub fn get_image_from_path(
        &mut self,
        path: &Path,
        recursive: &Option<Recursive>,
    ) -> Option<(PathBuf, usize)> {
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
                queue.push(img_path.clone());
            }
            (None | Some(ImagePickerAction::Previous), ImagePickerSorting::Random { .. }) => {}
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

    /// Update wallpaper by going down 1 index through the cached image paths
    /// Expiry timer reset even if already at the first cached image
    pub fn previous_image(&mut self) {
        self.action = Some(ImagePickerAction::Previous);
    }

    /// Update wallpaper by going up 1 index through the cached image paths
    pub fn next_image(&mut self, path: &Path, recursive: &Option<Recursive>) {
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
                    self.sorting = ImagePickerSorting::new_random(queue_size_for(
                        wallpaper_info,
                        &self.filelist_cache,
                    ));
                }
                (_, Sorting::Random) => {
                    // if the path was not changed, use the current image as the first image of
                    // the drawn_images
                    let mut queue =
                        Queue::with_capacity(queue_size_for(wallpaper_info, &self.filelist_cache));
                    queue.push(self.current_image());
                    self.sorting = ImagePickerSorting::Random(queue);
                }
                (_, Sorting::GroupedRandom { group }) if path_changed => {
                    self.sorting = ImagePickerSorting::GroupedRandom(GroupedRandom::new(
                        wallpaper_groups.clone(),
                        group,
                        wl_surface,
                        queue_size_for(wallpaper_info, &self.filelist_cache),
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
                        queue_size_for(wallpaper_info, &self.filelist_cache),
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
            self.sorting = ImagePickerSorting::new_random(queue_size_for(
                wallpaper_info,
                &self.filelist_cache,
            ));
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

    pub fn update_queue_size(&mut self, wallpaper_info: &WallpaperInfo) {
        let queue_size = queue_size_for(wallpaper_info, &self.filelist_cache);
        match &mut self.sorting {
            ImagePickerSorting::Random(queue) => {
                queue.resize(queue_size);
            }
            ImagePickerSorting::Ascending(_) | ImagePickerSorting::Descending(_) => {}
            ImagePickerSorting::GroupedRandom(group) => {
                group.group.borrow_mut().queue.resize(queue_size);
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

    // Pick uniformly among the images that have not been shown in the
    // current cycle. Sampling the complement directly means a repeat
    // can never happen by bad luck, only by exhaustion. The set keeps
    // this linear when the queue is sized to the whole collection.
    let shown: HashSet<&PathBuf> = queue.current_cycle().collect();
    let available: Vec<usize> = (0..files.len())
        .filter(|index| !shown.contains(&files[*index]))
        .collect();
    if !available.is_empty() {
        let index = available[fastrand::usize(..available.len())];
        return (index, files[index].to_path_buf());
    }

    // Every image has been shown: start a new cycle. The only image we
    // rule out is the one on screen, and we know there is more than one.
    queue.start_new_cycle();
    loop {
        let index = fastrand::usize(..files.len());
        if files[index] != current_image {
            break (index, files[index].to_path_buf());
        }
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

    use std::collections::HashSet;

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
    fn test_push_redisplayed_image() {
        // A queue larger than the image set forces the random fallback
        // to repeat an image we have already shown. Recording that
        // repeat moves it to the most recent slot, so previous walks
        // the order things actually appeared on screen.
        let mut queue = Queue::with_capacity(5);
        queue.push(PathBuf::from("mypath"));
        queue.push(PathBuf::from("mypath2"));
        queue.push(PathBuf::from("mypath3"));

        queue.push(PathBuf::from("mypath"));
        assert_eq!(Path::new("mypath"), queue.current());
        assert_eq!(Some((Path::new("mypath3"), 1)), queue.previous());
        assert_eq!(Some((Path::new("mypath2"), 0)), queue.previous());
        assert_eq!(None, queue.previous());
    }

    #[test]
    fn test_push_replayed_image() {
        // Walking back and then forward replays the queue in place.
        // Pushing a replayed image must leave the buffer and cursor
        // alone, or the replay would skip ahead of the user.
        let mut queue = Queue::with_capacity(3);
        queue.push(PathBuf::from("mypath"));
        queue.push(PathBuf::from("mypath2"));
        queue.push(PathBuf::from("mypath3"));

        assert_eq!(Some((Path::new("mypath2"), 1)), queue.previous());
        assert_eq!(Some((Path::new("mypath"), 0)), queue.previous());
        assert_eq!(Some((Path::new("mypath2"), 1)), queue.next());
        queue.push(PathBuf::from("mypath2"));
        assert_eq!(Some((Path::new("mypath3"), 2)), queue.next());
    }

    #[test]
    fn test_push_keeps_cycle_count() {
        // The cycle count follows where a re-shown image came from: one
        // shown again within the current cycle does not grow it, one
        // from before the cycle joins it.
        let mut queue = Queue::with_capacity(5);
        queue.push(PathBuf::from("mypath"));
        queue.push(PathBuf::from("mypath2"));
        queue.push(PathBuf::from("mypath3"));
        assert_eq!(3, queue.in_cycle);

        // mypath2 is already part of the cycle; showing it again is
        // not progress through the collection
        queue.push(PathBuf::from("mypath2"));
        assert_eq!(3, queue.in_cycle);
        assert_eq!(Path::new("mypath2"), queue.current());

        // A new cycle starts empty and the first image shown joins it
        queue.start_new_cycle();
        assert_eq!(0, queue.in_cycle);
        queue.push(PathBuf::from("mypath"));
        assert_eq!(1, queue.in_cycle);
    }

    #[test]
    fn test_resize_evicts_cursor() {
        // A live resize can fire while the user is anywhere in their
        // history. When the cursor's image is evicted, the cursor lands
        // on the oldest survivor.
        let mut queue = Queue::with_capacity(5);
        queue.push(PathBuf::from("mypath"));
        queue.push(PathBuf::from("mypath2"));
        queue.push(PathBuf::from("mypath3"));
        queue.push(PathBuf::from("mypath4"));
        queue.push(PathBuf::from("mypath5"));
        assert_eq!(Some((Path::new("mypath4"), 3)), queue.previous());
        assert_eq!(Some((Path::new("mypath3"), 2)), queue.previous());
        assert_eq!(Some((Path::new("mypath2"), 1)), queue.previous());

        queue.resize(2);
        assert_eq!(Path::new("mypath4"), queue.current());
        assert_eq!(None, queue.previous());
        assert_eq!(Some((Path::new("mypath5"), 1)), queue.next());
    }

    #[test]
    fn test_next_random_image_avoids_already_shown() {
        // With one image left unseen, the selection must pick it; chance
        // is not good enough. The loop guards against a regression to
        // sampling that only mostly avoids the queue.
        let files: Vec<PathBuf> = ["a", "b", "c", "d"].iter().map(PathBuf::from).collect();
        for _ in 0..50 {
            let mut queue = Queue::with_capacity(10);
            queue.push(files[0].clone());
            queue.push(files[1].clone());
            queue.push(files[2].clone());

            let (index, path) = next_random_image(&files[2], &mut queue, &files);
            assert_eq!(3, index);
            assert_eq!(files[3], path);
        }
    }

    #[test]
    fn test_next_random_image_cycles_without_repeats() {
        // Once every image has been shown, a new cycle starts: again no
        // repeats until every image has been shown, and no image twice
        // in a row across the boundary.
        let files: Vec<PathBuf> = ["a", "b", "c"].iter().map(PathBuf::from).collect();
        for _ in 0..20 {
            let mut queue = Queue::with_capacity(10);
            let mut current = PathBuf::new();
            for cycle in 0..3 {
                let mut shown = HashSet::new();
                for _ in 0..files.len() {
                    let (_, path) = next_random_image(&current, &mut queue, &files);
                    assert_ne!(current, path, "repeat across change in cycle {cycle}");
                    assert!(shown.insert(path.clone()), "repeat within cycle {cycle}");
                    queue.push(path.clone());
                    current = path;
                }
                assert_eq!(files.len(), shown.len());
            }
        }
    }

    #[test]
    fn test_effective_queue_size() {
        // An explicit queue-size always wins
        assert_eq!(7, effective_queue_size(Some(7), 100));
        // Without one, the queue covers the whole collection
        assert_eq!(100, effective_queue_size(None, 100));
        // Single files and empty folders fall back to the default
        assert_eq!(
            ImagePicker::DEFAULT_DRAWN_IMAGES_QUEUE_SIZE,
            effective_queue_size(None, 0)
        );
        // A queue needs room for at least one image
        assert_eq!(1, effective_queue_size(Some(0), 100));
    }

    #[test]
    fn test_resize_grow() {
        // Growing a queue that was not full left tail pointing at the
        // old last slot, and walking backwards past the oldest entry
        // indexed out of bounds.
        let mut queue = Queue::with_capacity(3);
        queue.push(PathBuf::from("mypath"));
        queue.push(PathBuf::from("mypath2"));
        queue.push(PathBuf::from("mypath3"));

        queue.resize(5);
        assert_eq!(Path::new("mypath3"), queue.current());
        assert_eq!(Some((Path::new("mypath2"), 1)), queue.previous());
        assert_eq!(Some((Path::new("mypath"), 0)), queue.previous());
        assert_eq!(None, queue.previous());
    }

    #[test]
    fn test_resize_shrink_not_full() {
        // Shrinking below the number of buffered entries must drop the
        // oldest ones; leaving the buffer bigger than the size means
        // eviction never starts again and navigation wraps wrongly.
        let mut queue = Queue::with_capacity(10);
        queue.push(PathBuf::from("mypath"));
        queue.push(PathBuf::from("mypath2"));
        queue.push(PathBuf::from("mypath3"));

        queue.resize(2);
        assert_eq!(Path::new("mypath3"), queue.current());
        assert_eq!(Some((Path::new("mypath2"), 0)), queue.previous());
        assert_eq!(None, queue.previous());
    }

    #[test]
    fn test_resize_to_zero() {
        // A queue cannot work with zero capacity; an automatically sized
        // queue can ask for it when its folder is emptied. Ignore it.
        let mut queue = Queue::with_capacity(2);
        queue.push(PathBuf::from("mypath"));
        queue.push(PathBuf::from("mypath2"));

        queue.resize(0);
        assert_eq!(Path::new("mypath2"), queue.current());
        assert_eq!(Some((Path::new("mypath"), 0)), queue.previous());
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
