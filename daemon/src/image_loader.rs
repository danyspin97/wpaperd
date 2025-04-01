use std::{
    collections::HashMap,
    path::PathBuf,
    sync::mpsc::{Receiver, TryRecvError},
};

use color_eyre::eyre::eyre;
use image::{open, RgbaImage};
use log::warn;
use smithay_client_toolkit::reexports::calloop::ping::Ping;

type ImageData = Option<RgbaImage>;

struct Image {
    data: ImageData,
    thread_handle: Option<Receiver<ImageData>>,
    requesters: Vec<String>,
}

pub enum ImageLoaderStatus {
    Loaded(RgbaImage),
    Waiting,
    Error,
}

pub struct ImageLoader {
    images: HashMap<PathBuf, Image>,
    ping: Ping,
}

impl ImageLoader {
    pub fn new(ping: Ping) -> Self {
        Self {
            images: HashMap::new(),
            ping,
        }
    }

    pub fn background_load(&mut self, path: PathBuf, requester_name: String) -> ImageLoaderStatus {
        if let Some(image) = self.images.get_mut(&path) {
            if let Some(rx) = image.thread_handle.take() {
                match rx.try_recv() {
                    Ok(Some(image_data)) => {
                        image.data = Some(image_data);
                    }
                    Ok(None) | Err(TryRecvError::Disconnected) => {
                        self.images.remove(&path);
                        return ImageLoaderStatus::Error;
                    }
                    Err(TryRecvError::Empty) => {
                        // the thread is still running
                        // reassign the handle
                        image.thread_handle = Some(rx);
                        // if this is a new requester, add it to the list
                        if !image.requesters.contains(&requester_name) {
                            image.requesters.push(requester_name);
                        }
                        return ImageLoaderStatus::Waiting;
                    }
                }
            }
            if let Some(data) = &image.data {
                // If the requesters is only one and it's the same as the current
                if image.requesters.len() == 1
                    && image.requesters.first().unwrap() == &requester_name
                {
                    // Just send it up and remove it from the map
                    let image = self.images.remove(&path);
                    ImageLoaderStatus::Loaded(image.unwrap().data.unwrap())
                } else {
                    // otherwise this image has been requested by multiple surfaces
                    let requesters = &mut image.requesters;
                    if let Some(index) = requesters.iter().position(|name| name == &requester_name)
                    {
                        requesters.remove(index);
                    }
                    ImageLoaderStatus::Loaded(data.clone())
                }
            } else {
                // The decoded image is not ready yet
                ImageLoaderStatus::Waiting
            }
        } else {
            self.start_new_thread(path, requester_name);
            ImageLoaderStatus::Waiting
        }
    }

    fn start_new_thread(&mut self, path: PathBuf, requester_name: String) {
        // Start loading a new image in a new thread
        let path_clone = path.clone();
        let ping_clone = self.ping.clone();
        let requester_clone = requester_name.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        rayon::spawn(move || match open(&path_clone) {
            Ok(image) => {
                // Notify the event loop that the image has been loaded
                // We need this so that Surface::load_wallpaper is called even if
                // wl_surface::frame doesn't get called by the compositor (e.g. a window is
                // fullscreen)
                // Do the conversion first, then the ping, otherwise we will have a race
                // condition
                let image = image.into_rgba8();
                tx.send(Some(image)).unwrap();
                ping_clone.ping();
            }
            Err(err) => {
                warn!(
                    "{:?}",
                    eyre!(err).wrap_err(format!(
                        "Failed to read image {path_clone:?} needed for {requester_clone}"
                    ))
                );
                tx.send(None).unwrap();
            }
        });
        let image = Image {
            requesters: vec![requester_name],
            thread_handle: Some(rx),
            data: None,
        };
        self.images.insert(path, image);
    }

    /// Check that there are no threads waiting on zero requesters
    #[cfg(debug_assertions)]
    pub fn check_lingering_threads(&mut self) {
        debug_assert!(!self
            .images
            .iter()
            .any(|(_, image)| { image.requesters.is_empty() }));
    }
}
