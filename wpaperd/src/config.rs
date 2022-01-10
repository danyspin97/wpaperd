use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use color_eyre::{eyre::ensure, Result};
use serde::Deserialize;

use crate::output::Output;

#[derive(Deserialize)]
pub struct Config {
    #[serde(flatten)]
    data: HashMap<String, Arc<Output>>,
    #[serde(skip)]
    default_config: Arc<Output>,
    #[serde(skip)]
    pub path: PathBuf,
    #[serde(skip)]
    pub reloaded: bool,
}

impl Config {
    pub fn new_from_path(path: &Path) -> Result<Self> {
        ensure!(
            path.exists(),
            "Configuration file {:?} does not exists",
            path
        );
        let mut config_manager: Self = toml::from_str(&fs::read_to_string(path)?)?;
        config_manager.default_config = config_manager
            .data
            .get("default")
            .unwrap_or(&Arc::new(Output::default()))
            .clone();
        for (name, config) in &config_manager.data {
            let path = config.path.as_ref().unwrap();
            ensure!(
                path.exists(),
                "File or directory {:?} for input {} does not exist",
                path,
                name
            );
            ensure!(
                config.duration.is_none() || path.is_dir(),
                "Duration can only be set when path points to a directory, for input {}",
                name
            );
        }

        config_manager.path = path.to_path_buf();
        config_manager.reloaded = false;
        Ok(config_manager)
    }

    pub fn get_output_by_name(&self, name: &str) -> Arc<Output> {
        self.data.get(name).unwrap_or(&self.default_config).clone()
    }
}
