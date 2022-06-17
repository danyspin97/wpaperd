use dirs::home_dir;
use std::{path::Path, path::PathBuf, time::Duration};

use serde::Deserialize;

#[derive(Default, Deserialize)]
pub struct Output {
    #[serde(deserialize_with = "tilde_expansion_deserialize")]
    pub path: Option<PathBuf>,
    pub mode: Option<()>,
    #[serde(default, with = "humantime_serde")]
    pub duration: Option<Duration>,
    #[serde(rename = "apply-shadow")]
    pub apply_shadow: Option<bool>,
}

pub fn tilde_expansion_deserialize<'de, D>(deserializer: D) -> Result<Option<PathBuf>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let path = String::deserialize(deserializer)?;
    let path = Path::new(&path);

    Ok(Some(
        path.strip_prefix("~")
            .map_or(path.to_path_buf(), |p| home_dir().unwrap().join(p)),
    ))
}
