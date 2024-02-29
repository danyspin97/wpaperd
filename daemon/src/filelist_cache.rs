use std::{
    path::{Path, PathBuf},
    sync::{atomic::AtomicBool, Arc},
};

use color_eyre::eyre::Context;
use hotwatch::Hotwatch;
use log::error;
use walkdir::WalkDir;

struct Filelist {
    path: PathBuf,
    filelist: Arc<Vec<PathBuf>>,
    outdated: Arc<AtomicBool>,
}

impl Filelist {
    fn new(path: &Path) -> Self {
        let mut res = Self {
            path: path.to_path_buf(),
            filelist: Arc::new(Vec::new()),
            outdated: Arc::new(AtomicBool::new(true)),
        };
        res.populate();
        res
    }
    fn populate(&mut self) {
        self.filelist = Arc::new(
            WalkDir::new(&self.path)
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
                .collect(),
        );
    }
}

pub struct FilelistCache {
    cache: Vec<Filelist>,
}

impl FilelistCache {
    pub fn new() -> Self {
        Self { cache: Vec::new() }
    }

    pub fn get(&self, path: &Path) -> Arc<Vec<PathBuf>> {
        self.cache
            .iter()
            .find(|filelist| filelist.path == path)
            .expect("path passed to Filelist::get has been cached")
            .filelist
            .clone()
    }

    /// paths must be sorted
    pub fn update_paths(&mut self, paths: Vec<PathBuf>, hotwatch: &mut Hotwatch) {
        self.cache.retain(|filelist| {
            if paths.contains(&filelist.path) {
                true
            } else {
                // Stop watching paths that have been removed
                if let Err(err) = hotwatch
                    .unwatch(&filelist.path)
                    .with_context(|| format!("hotwatch unwatch error on path {:?}", &filelist.path))
                {
                    error!("{err:?}");
                }
                // and remove them from the vec
                false
            }
        });

        for path in paths {
            if !self.cache.iter().any(|filelist| filelist.path == path) {
                self.cache.push(Filelist::new(&path));
                if let Err(err) = hotwatch
                    .watch(&path, |event| match event.kind {
                        hotwatch::EventKind::Create(_) | hotwatch::EventKind::Remove(_) => {}
                        _ => {}
                    })
                    .with_context(|| format!("hotwatch watch error on path {:?}", &path))
                {
                    error!("{err:?}");
                }
            }
        }

        self.update();
    }

    pub fn update(&mut self) {
        for filelist in &mut self.cache {
            if filelist.outdated.load(std::sync::atomic::Ordering::Relaxed) {
                filelist.populate();
            }
        }
    }
}
