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
    eyre::{ensure, eyre, WrapErr},
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
    render::Transition,
    wallpaper_info::{BackgroundMode, Recursive, Sorting, WallpaperInfo},
};

use std::os::unix::fs::PermissionsExt;

#[derive(Default, Deserialize, PartialEq, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct SerializedWallpaperInfo {
    #[serde(default, deserialize_with = "tilde_expansion_deserialize")]
    pub path: Option<PathBuf>,
    #[serde(default, with = "humantime_serde")]
    pub duration: Option<Duration>,
    #[serde(rename = "apply-shadow")]
    pub apply_shadow: Option<bool>,
    pub sorting: Option<Sorting>,
    pub mode: Option<BackgroundMode>,
    #[serde(rename = "queue-size")]
    pub queue_size: Option<usize>,
    #[serde(rename = "transition-time")]
    pub transition_time: Option<u32>,

    /// Determines if we should show the transition between black and first
    /// wallpaper. `Some(false)` means we instantly cut to the first wallpaper,
    /// `Some(true)` means we fade from black to the first wallpaper.
    ///
    /// See [crate::wallpaper_info::WallpaperInfo]
    #[serde(rename = "initial-transition")]
    pub initial_transition: Option<bool>,
    pub transition: Option<Transition>,

    /// Determine the offset for the wallpaper to be drawn into the screen
    /// Must be from 0.0 to 1.0, by default is 0.0 in tile mode and 0.5 in all the others
    ///
    /// See [crate::wallpaper_info::WallpaperInfo]
    pub offset: Option<f32>,

    /// Assign these displays to a group that shows the same wallpaper
    pub group: Option<u8>,

    /// Recursively traverse the directory set as path
    /// Set as true by default
    pub recursive: Option<bool>,

    // Path to bash script.
    #[serde(default, deserialize_with = "tilde_expansion_deserialize")]
    pub exec: Option<PathBuf>,
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
                return Err(eyre!(
                    "Attribute {} must be set",
                    "path".bold().italic().blue(),
                ))
                .with_suggestion(|| {
                    format!(
                        "Add attribute {} in the display section of the configuration:\npath = \"</path/to/image>\"",
                        "path".bold().italic().blue(),
                    )
                });
            }
        }
        .to_path_buf();
        // Ensure that a path exists
        if !path.exists() {
            return Err(eyre!(
                "Path {} for attribute {}{} must exist",
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
                    "Set attribute {} to an existing file or directory",
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
            return Err(eyre!(
                "{} cannot be set when {} points to a file",
                "duration".bold().italic().blue(),
                "path".bold().italic().blue(),
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
            (None, Some(_)) if path.is_file() && !path_inherited => None,
            (Some(sorting), _) | (None, Some(sorting)) => Some(*sorting),
            (None, None) => None,
        };

        let group = match (&self.group, &default.group) {
            (None, Some(_)) if path.is_file() && !path_inherited => None,
            (Some(sorting), _) | (None, Some(sorting)) => Some(*sorting),
            (None, None) => None,
        };

        // sorting and group can only be set when path is a directory
        if (sorting.is_some() || group.is_some()) && !path.is_dir() {
            // Do no use bail! to add suggestion
            return Err(eyre!(
                "{} cannot be set when {} is a directory",
                if sorting.is_some() {
                    "sorting"
                } else {
                    "group"
                }
                .bold()
                .italic()
                .blue(),
                "path".bold().italic().blue(),
            )
            .with_suggestion(|| {
                format!(
                    "Either remove {} or set {} to a directory",
                    if sorting.is_some() {
                        "sorting"
                    } else {
                        "group"
                    }
                    .bold()
                    .italic()
                    .blue(),
                    "path".bold().italic().blue(),
                )
            }));
        }

        // If there is no sorting but the group is set
        let sorting = if group.is_some() && sorting.is_none() {
            // Assign it the default one, so that we can do the group match below
            Some(Sorting::default())
        } else {
            sorting
        };
        let sorting = sorting.map(|sorting| {
            if let Some(group) = group {
                match sorting {
                    Sorting::Random => Sorting::GroupedRandom { group },
                    Sorting::Ascending => todo!(),
                    Sorting::Descending => todo!(),
                    Sorting::GroupedRandom { group: _ } => unreachable!(),
                }
            } else {
                sorting
            }
        });

        let mode = match (&self.mode, &default.mode) {
            (Some(mode), _) | (None, Some(mode)) => *mode,
            (None, None) => BackgroundMode::default(),
        };
        let drawn_images_queue_size = match (&self.queue_size, &default.queue_size) {
            (Some(size), _) | (None, Some(size)) => *size,
            (None, None) => ImagePicker::DEFAULT_DRAWN_IMAGES_QUEUE_SIZE,
        };
        let initial_transition = match (&self.initial_transition, &default.initial_transition) {
            (Some(initial_transition), _) | (None, Some(initial_transition)) => *initial_transition,
            (None, None) => true,
        };

        let transition = match (&self.transition, &default.transition) {
            (Some(transition), _) | (None, Some(transition)) => transition.clone(),
            (None, None) => Transition::Fade {},
        };

        let transition_time = match (&self.transition_time, &default.transition_time) {
            (Some(transition_time), _) | (None, Some(transition_time)) => *transition_time,
            (None, None) => transition.default_transition_time(),
        };

        let offset = match (&self.offset, &default.offset) {
            (Some(offset), _) | (None, Some(offset)) => Some(*offset),
            (None, None) => None,
        };

        let recursive = match (&self.recursive, &default.recursive) {
            (Some(recursive), _) | (None, Some(recursive)) => {
                Some(std::convert::Into::<Recursive>::into(*recursive))
            }
            (None, None) => None,
        };

        if let Some(exec_path) = &self.exec {
            ensure!(
                exec_path.exists(),
                "Exec script {} must exist",
                exec_path.to_string_lossy().italic().yellow()
            );
            ensure!(
                exec_path.is_file(),
                "Exec path {} must be a file",
                exec_path.to_string_lossy().italic().yellow()
            );
            ensure!(
                std::fs::metadata(exec_path)?.permissions().mode() & 0o111 != 0,
                "Exec script {} must be executable",
                exec_path.to_string_lossy().italic().yellow()
            );
        }

        Ok(WallpaperInfo {
            path,
            duration,
            apply_shadow: false,
            sorting,
            mode,
            drawn_images_queue_size,
            transition_time,
            initial_transition,
            transition,
            offset,
            recursive,
            exec: self.exec.clone(),
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
        ensure!(path.exists(), "File {path:?} does not exist");
        let mut config: Self = toml::from_str(&fs::read_to_string(path)?)?;
        config
            .data
            .get("default")
            .unwrap_or(&SerializedWallpaperInfo::default())
            .clone_into(&mut config.default);
        config
            .data
            .get("any")
            .unwrap_or(&SerializedWallpaperInfo::default())
            .clone_into(&mut config.any);
        config.data.retain(|name, info| {
            // The default configuration does not follow these rules
            // We still need the default configuration here because the path needs to be cached
            if info == &config.default {
                true
            } else {
                match info.apply_and_validate(&config.default).wrap_err_with(|| {
                    format!(
                        "Failed to validate configuration for display {}",
                        name.bold().magenta()
                    )
                }) {
                    Ok(_) => true,
                    Err(err) => {
                        // We do not want to exit when error occurs, print it and go forward
                        warn!("{err:?}");
                        false
                    }
                }
            }
        });

        let groups = config
            .data
            .iter()
            .filter_map(|(name, info)| {
                // It's a bit overkill to call this function again, but the displays are on average 1 or 2
                // and the code is simple enough that it wouldn't make a difference with 10 either
                info.apply_and_validate(&config.default)
                    .map(|res| (name, res))
                    .ok()
            })
            .filter(|(_, info)| {
                info.sorting.is_some()
                    && matches!(info.sorting.unwrap(), Sorting::GroupedRandom { .. })
            })
            .collect::<Vec<_>>();

        // Check if all the groups share the same path
        // This check is only useful when there is more than one display
        // We display a warning related to the config issue, but we are not fixing anything here
        // The path that will be used is probably not what the user expect, but proving an "expected"
        // behaviour for a configuration issue seems overkill
        if groups.len() > 1 {
            // We want to skip displays for which we already displayed the warning
            let mut errored_list = Vec::new();
            for i in 0..groups.len() {
                let x = groups.get(i).unwrap();
                for j in 1..groups.len() {
                    if errored_list.contains(&j) {
                        continue;
                    }
                    let y = groups.get(j).unwrap();
                    if !(x.1.sorting.is_none()
                        || x.1.sorting != y.1.sorting
                        || x.1.path == y.1.path)
                    {
                        warn!(
                            "Displays {} and {} are assigned to group {} but have different paths",
                            x.0,
                            y.0,
                            match x.1.sorting.unwrap() {
                                Sorting::GroupedRandom { group } => group,
                                _ => unreachable!(),
                            }
                        );
                        errored_list.push(j);
                    }
                }
            }
        }

        config.path = path.to_path_buf();
        Ok(config)
    }

    pub fn get_info_for_output(&self, name: &str, description: &str) -> Result<WallpaperInfo> {
        use regex::Regex;
        // Actually, wayland may report an output description in different
        // formats as the compositors wants to. For example, niri reports
        // description in format `<vendor name> - <monitor name> - <port name>`
        // so, we can not handle here the all formats at once. But users
        // can use a regex to match theirs' compositors outputs.
        let mut matched = None;
        for (k, v) in self.data.iter() {
            if !k.starts_with("re:") {
                continue;
            }
            // TODO(Shvedov): Better to compile re withing creation of
            // WallpaperInfo, but adding field of type `Option<Regex>` breaks
            // PatialEq macro.
            let re = Regex::new(&k[3..]).unwrap();
            if re.is_match(description) {
                matched = Some(v);
                break;
            };
        }

        if let Some(info) = matched {
            info.apply_and_validate(&self.default)
        } else {
            let mut cleaned = String::from(description);

            // Wayland may report an output description that includes
            // information about the port and port type.  This information
            // is *not* reported by sway so we need to strip it off so
            // outputs are matched the way users expect.
            if let Some(offset) = cleaned.rfind(" (") {
                cleaned.truncate(offset);
            }

            self.data
                .get(&cleaned)
                .or_else(|| self.data.get(name))
                .unwrap_or(&self.any)
                .apply_and_validate(&self.default)
        }
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
            .wrap_err_with(|| format!("Failed to watch file changes for {:?}", &self.path))?;
        Ok(())
    }

    pub fn paths(&self) -> Vec<(PathBuf, Recursive)> {
        let mut paths: Vec<_> = self
            .data
            .values()
            .filter_map(|info| {
                info.path.as_ref().map(|p| {
                    (
                        p.to_path_buf(),
                        info.recursive.map(Recursive::from).unwrap_or_default(),
                    )
                })
            })
            .collect();
        paths.sort_unstable();
        paths.dedup();
        paths
    }

    /// Return true if the struct changed
    pub fn update(&mut self) -> bool {
        // When the config file has been written into
        let new_config = Config::new_from_path(&self.path).wrap_err_with(|| {
            format!(
                "Failed to read the new configuration from {}",
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
