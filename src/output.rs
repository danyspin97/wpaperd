use dirs::home_dir;
use std::{path::Path, path::PathBuf, time::Duration};

use serde::Deserialize;

#[derive(Default, Deserialize)]
pub struct Output {
    #[serde(deserialize_with = "path")]
    pub path: Option<PathBuf>,
    pub mode: Option<()>,
    #[serde(default, with = "humantime_serde")]
    pub duration: Option<Duration>,
    #[serde(rename = "apply-shadow")]
    pub apply_shadow: Option<bool>,
}

pub fn path<'de, D>(deserializer: D) -> Result<Option<PathBuf>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let mut path = String::deserialize(deserializer)?;
    if path.starts_with("~/") {
        let home = home_dir().unwrap();
        path = path.replacen('~', home.to_str().unwrap(), 1);
    }

    let mut pathbuf = PathBuf::new();
    pathbuf.push(Path::new(&path));
    Ok(Some(pathbuf))
}
