use std::{collections::HashMap, fs, path::Path, sync::Arc};

use color_eyre::{eyre::ensure, Result};
use serde::Deserialize;

use crate::output::Output;

#[derive(Deserialize)]
pub struct Config {
    #[serde(flatten)]
    data: HashMap<String, Arc<Output>>,
    #[serde(skip)]
    default_config: Arc<Output>,
}

impl Config {
    pub fn new_from_path(path: &Path) -> Result<Self> {
        let mut config_manager: Self = serde_yaml::from_str(&fs::read_to_string(path)?)?;
        config_manager.default_config = config_manager
            .data
            .get("*")
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
                config.time.is_none() || path.is_dir(),
                "Time can only be set when path points to a directory, for input {}",
                name
            );
        }

        Ok(config_manager)
    }

    pub fn get_output_by_name(&self, name: &str) -> Arc<Output> {
        self.data.get(name).unwrap_or(&self.default_config).clone()
    }
}
