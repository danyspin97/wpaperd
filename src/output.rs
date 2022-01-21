use std::{path::PathBuf, time::Duration};

use serde::Deserialize;

#[derive(Default, Deserialize)]
pub struct Output {
    pub path: Option<PathBuf>,
    pub mode: Option<()>,
    #[serde(default, with = "humantime_serde")]
    pub duration: Option<Duration>,
}
