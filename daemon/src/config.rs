use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use color_eyre::{
    eyre::{anyhow, ensure, Context},
    owo_colors::OwoColorize,
    Result, Section,
};
use dirs::home_dir;
use hotwatch::{Event, Hotwatch};
use log::{error, warn};
use serde::Deserialize;
use smithay_client_toolkit::reexports::calloop::ping::Ping;

use crate::{
    image_picker::ImagePicker,
    render::Renderer,
    wallpaper_info::{BackgroundMode, Sorting, WallpaperInfo},
};

#[derive(Default, Deserialize, PartialEq, Debug, Clone)]
pub struct SerializedWallpaperInfo {
    #[serde(default, deserialize_with = "tilde_expansion_deserialize")]
    pub path: Option<PathBuf>,
    #[serde(default, with = "humantime_serde")]
    pub duration: Option<Duration>,
    #[serde(rename = "apply-shadow")]
    pub apply_shadow: Option<bool>,
    pub sorting: Option<Sorting>,
    pub mode: Option<BackgroundMode>,
    pub queue_size: Option<usize>,
    pub transition_time: Option<u32>,
    
    /// Determines if we should show the transition between black and first 
    /// wallpaper. `Some(false)` means we instantly cut to the first wallpaper,
    /// `Some(true)` means we fade from black to the first wallpaper.
    ///
    /// See [crate::wallpaper_info::WallpaperInfo]
    pub initial_transition: Option<bool>,
}

impl SerializedWallpaperInfo {
    pub fn apply_and_validate(&self, default: &Self) -> Result<WallpaperInfo> {
        let mut path_inherited = false;
        let path = match (&self.path, &default.path) {
            (Some(path), None) | (Some(path), Some(_))=> path,
            (None, Some(path)) => {
                path_inherited = true;
                path
            }
            (None, None) => {
                return Err(anyhow!(
                    "attribute {} is not set",
                    "path".bold().italic().blue(),
                ))
                .with_suggestion(|| {
                    format!(
                        "add attribute {} in the display section of the configuration:\npath = \"</path/to/image>\"",
                        "path".bold().italic().blue(),
                    )
                });
            }
        }
        .to_path_buf();
        // Ensure that a path exists
        if !path.exists() {
            return Err(anyhow!(
                "path {} for attribute {}{} does not exist",
                path.to_string_lossy().italic().yellow(),
                "path".bold().italic().blue(),
                if path_inherited {
                    format!(
                        " (inherited from {} configuration)",
                        "default".magenta().bold()
                    )
                } else {
                    "".to_string()
                }
            ))
            .with_suggestion(|| {
                format!(
                    "set attribute {} to an existing file or directory",
                    "path".bold().italic().blue(),
                )
            });
        }

        let duration = match (&self.duration, &default.duration) {
            // duration is inherited from default, but this section set path to a file, ignore
            // duration
            (None, Some(_)) if path.is_file() && !path_inherited => None,
            (Some(duration), _) | (None, Some(duration)) => Some(*duration),
            (None, None) => None,
        };
        // duration can only be set when path is a directory
        if duration.is_some() && !path.is_dir() {
            // Do no use bail! to add suggestion
            return Err(anyhow!(
                "Attribute {} is set to a file and attribute {} is also set.",
                "path".bold().italic().blue(),
                "duration".bold().italic().blue()
            )
            .with_suggestion(|| {
                format!(
                    "Either remove {} or set {} to a directory",
                    "path".bold().italic().blue(),
                    "duration".bold().italic().blue()
                )
            }));
        }

        let sorting = match (&self.sorting, &default.sorting) {
            (Some(sorting), _) | (None, Some(sorting)) => *sorting,
            (None, None) => Sorting::default(),
        };
        let mode = match (&self.mode, &default.mode) {
            (Some(mode), _) | (None, Some(mode)) => *mode,
            (None, None) => BackgroundMode::default(),
        };
        let drawn_images_queue_size = match (&self.queue_size, &default.queue_size) {
            (Some(size), _) | (None, Some(size)) => *size,
            (None, None) => ImagePicker::DEFAULT_DRAWN_IMAGES_QUEUE_SIZE,
        };
        let transition_time = match (&self.transition_time, &default.transition_time) {
            (Some(transition_time), _) | (None, Some(transition_time)) => *transition_time,
            (None, None) => Renderer::DEFAULT_TRANSITION_TIME,
        };
        let initial_transition = match (&self.initial_transition, &default.initial_transition) {
            (Some(initial_transition), _) | (None, Some(initial_transition)) => *initial_transition,
            (None, None) => true,
        };

        Ok(WallpaperInfo {
            path,
            duration,
            apply_shadow: false,
            sorting,
            mode,
            drawn_images_queue_size,
            transition_time,
            initial_transition
        })
    }
}

#[derive(Deserialize, Default)]
pub struct Config {
    #[serde(flatten)]
    data: HashMap<String, SerializedWallpaperInfo>,
    #[serde(skip)]
    default: SerializedWallpaperInfo,
    #[serde(skip)]
    any: SerializedWallpaperInfo,
    #[serde(skip)]
    pub path: PathBuf,
    #[serde(skip)]
    pub reloaded: Option<Arc<AtomicBool>>,
}

impl Config {
    pub fn new_from_path(path: &Path) -> Result<Self> {
        ensure!(path.exists(), "File {path:?} does not exists");
        let mut config: Self = toml::from_str(&fs::read_to_string(path)?)?;
        config.default = config
            .data
            .get("default")
            .unwrap_or(&SerializedWallpaperInfo::default())
            .to_owned();
        config.any = config
            .data
            .get("any")
            .unwrap_or(&SerializedWallpaperInfo::default())
            .to_owned();
        config.data.retain(|name, info| {
            // The default configuration does not follow these rules
            // We still need the default configuration here because the path needs to be cached
            if info == &config.default {
                true
            } else {
                match info
                    .apply_and_validate(&config.default)
                    .with_context(|| format!("while validating display {}", name.bold().magenta()))
                {
                    Ok(_) => true,
                    Err(err) => {
                        // We do not want to exit when error occurs, print it and go forward
                        warn!("{err:?}");
                        false
                    }
                }
            }
        });

        config.path = path.to_path_buf();
        Ok(config)
    }

    pub fn get_output_by_name(&self, name: &str) -> Result<WallpaperInfo> {
        self.data
            .get(name)
            .unwrap_or(&self.any)
            .apply_and_validate(&self.default)
    }

    pub fn listen_to_changes(&self, hotwatch: &mut Hotwatch, ping: Ping) -> Result<()> {
        let reloaded = self.reloaded.as_ref().unwrap().clone();
        hotwatch
            .watch(&self.path, move |event: Event| {
                if let hotwatch::EventKind::Modify(_) = event.kind {
                    reloaded.store(true, Ordering::Relaxed);
                    ping.ping();
                }
            })
            .with_context(|| format!("watching file {:?}", &self.path))?;
        Ok(())
    }

    pub fn paths(&self) -> Vec<PathBuf> {
        let mut paths: Vec<_> = self
            .data
            .values()
            .filter_map(|info| info.path.as_ref().map(|p| p.to_path_buf()))
            .collect();
        paths.sort_unstable();
        paths.dedup();
        paths
    }

    /// Return true if the struct changed
    pub fn update(&mut self) -> bool {
        // When the config file has been written into
        let new_config = Config::new_from_path(&self.path).with_context(|| {
            format!(
                "updating configuration from file {}",
                self.path.to_string_lossy()
            )
        });
        match new_config {
            Ok(new_config) if new_config != *self => {
                let reloaded = self.reloaded.as_ref().unwrap().clone();
                *self = new_config;
                self.reloaded = Some(reloaded);
                true
            }
            Ok(_) => {
                // Do nothing, the new config is the same as the loaded one
                false
            }
            Err(err) => {
                error!("{err:?}");
                false
            }
        }
    }
}

impl PartialEq for Config {
    fn eq(&self, other: &Self) -> bool {
        self.data == other.data
    }
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
