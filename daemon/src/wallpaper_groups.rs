use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    path::PathBuf,
    rc::Rc,
};

use smithay_client_toolkit::reexports::client::{protocol::wl_surface::WlSurface, QueueHandle};

use crate::{image_picker::Queue, wpaperd::Wpaperd};

pub struct WallpaperGroup {
    pub group: u8,
    pub index: usize,
    pub current_image: PathBuf,
    pub loading_image: Option<(usize, PathBuf)>,
    pub surfaces: HashSet<WlSurface>,
    pub queue: Queue,
}

impl WallpaperGroup {
    pub fn new(group: u8, queue_size: usize) -> Self {
        Self {
            group,
            index: 0,
            current_image: PathBuf::from(""),
            loading_image: None,
            surfaces: HashSet::new(),
            queue: Queue::with_capacity(queue_size),
        }
    }

    pub fn queue_all_surfaces(&self, qh: &QueueHandle<Wpaperd>) {
        for surface in &self.surfaces {
            surface.frame(qh, surface.clone());
            surface.commit();
        }
    }
}

pub struct WallpaperGroups {
    groups: HashMap<u8, Rc<RefCell<WallpaperGroup>>>,
}

impl WallpaperGroups {
    pub fn new() -> Self {
        Self {
            groups: HashMap::new(),
        }
    }

    pub fn get_or_insert(
        &mut self,
        group: u8,
        wl_surface: &WlSurface,
        queue_size: usize,
    ) -> Rc<RefCell<WallpaperGroup>> {
        self.groups
            .entry(group)
            .or_insert_with(|| Rc::new(RefCell::new(WallpaperGroup::new(group, queue_size))));
        let wp_group = self.groups.get_mut(&group).unwrap();
        wp_group.borrow_mut().surfaces.insert(wl_surface.clone());
        wp_group.clone()
    }

    pub fn remove(&mut self, group: u8, wl_surface: &WlSurface) {
        let wp_group = self.groups.get(&group).unwrap();
        let mut wp_group = wp_group.borrow_mut();
        // if this wl_surface is the last one for this WallpaperGroup
        if wp_group.surfaces.len() == 1 {
            drop(wp_group);
            // Remove it from the WallpaperGroups
            self.groups.remove(&group);
        } else {
            wp_group.surfaces.remove(wl_surface);
        }
    }
}
