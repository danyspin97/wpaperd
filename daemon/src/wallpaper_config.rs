use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use color_eyre::{eyre::ensure, Result};
use serde::Deserialize;

use crate::wallpaper_info::WallpaperInfo;

#[derive(Deserialize)]
pub struct WallpaperConfig {
    #[serde(flatten)]
    data: HashMap<String, Arc<WallpaperInfo>>,
    #[serde(skip)]
    default_config: Arc<WallpaperInfo>,
    #[serde(skip)]
    pub path: PathBuf,
    #[serde(skip)]
    pub reloaded: bool,
}

impl WallpaperConfig {
    pub fn new_from_path(path: &Path) -> Result<Self> {
        ensure!(path.exists(), "Configuration file {path:?} does not exists",);
        let mut config_manager: Self = toml::from_str(&fs::read_to_string(path)?)?;
        config_manager.default_config = config_manager
            .data
            .get("default")
            .unwrap_or(&Arc::new(WallpaperInfo::default()))
            .clone();
        for (name, config) in &config_manager.data {
            let path = config.path.as_ref().unwrap();
            ensure!(
                path.exists(),
                "File or directory {path:?} for input {name} does not exist"
            );
            ensure!(
                config.duration.is_none() || path.is_dir(),
                "for input '{name}', `path` is set to an image but `duration` is also set.
Either remove `duration` or set `path` to a directory"
            );
        }

        config_manager.path = path.to_path_buf();
        config_manager.reloaded = true;
        Ok(config_manager)
    }

    pub fn get_output_by_name(&self, name: &str) -> Arc<WallpaperInfo> {
        self.data.get(name).unwrap_or(&self.default_config).clone()
    }
}

impl PartialEq for WallpaperConfig {
    fn eq(&self, other: &Self) -> bool {
        self.data == other.data
    }
}
