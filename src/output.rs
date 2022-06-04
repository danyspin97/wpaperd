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
    let path = String::deserialize(deserializer)?;
    match shellexpand::full(&path) {
        Ok(path) => {
            let mut pathbuf = PathBuf::new();
            pathbuf.push(Path::new(&*path));
            Ok(Some(pathbuf))
        }
        Err(_) => Ok(None),
    }
}
