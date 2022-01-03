use std::path::PathBuf;

use serde::Deserialize;

#[derive(Default, Deserialize)]
pub struct Output {
    pub path: Option<PathBuf>,
    pub mode: Option<()>,
    pub time: Option<u32>,
}
