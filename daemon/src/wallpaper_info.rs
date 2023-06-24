use dirs::home_dir;
use std::{path::Path, path::PathBuf, time::Duration};

use serde::Deserialize;

#[derive(Default, Deserialize, PartialEq)]
pub struct WallpaperInfo {
    #[serde(deserialize_with = "tilde_expansion_deserialize")]
    pub path: Option<PathBuf>,
    pub mode: Option<()>,
    #[serde(default, with = "humantime_serde")]
    pub duration: Option<Duration>,
    #[serde(rename = "apply-shadow")]
    pub apply_shadow: Option<bool>,
    #[serde(default = "set_sorting_default", deserialize_with = "order_deserializer")]
    pub sorting: Option<String>
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

fn set_sorting_default() -> Option<String> {
    Some("random".to_string())
}

pub fn order_deserializer<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let natural = String::from("natural");
    let reverse = String::from("reverse");
    let random = String::from("random");
    let order = String::deserialize(deserializer)?;

    // Default to random sorting if we don't match reverse or natural
    let result = if order.eq(&reverse) {
        reverse
    } else if order.eq(&natural) {
        natural
    } else {
        random
    };

    Ok(Some(result))
}
