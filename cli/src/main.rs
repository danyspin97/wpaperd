mod opts;

use std::{
    io::{Read, Write},
    os::unix::net::UnixStream,
    path::PathBuf,
    time::Duration,
};

use clap::Parser;
use serde::Serialize;
use wpaperd_ipc::{socket_path, IpcError, IpcMessage, IpcResponse};

use crate::opts::{Opts, SubCmd};

fn unquote(s: String) -> String {
    if s.starts_with('"') && s.ends_with('"') {
        s.trim_start_matches('"').trim_end_matches('"').to_string()
    } else {
        s
    }
}

fn main() {
    let args = Opts::parse();

    let mut json_resp = false;

    let mut conn = UnixStream::connect(socket_path().unwrap()).unwrap();
    let msg = match args.subcmd {
        SubCmd::GetWallpaper { monitor } => IpcMessage::CurrentWallpaper {
            monitor: unquote(monitor),
        },
        SubCmd::AllWallpapers { json } => {
            json_resp = json;
            IpcMessage::AllWallpapers
        }
        SubCmd::NextWallpaper { monitors } => IpcMessage::NextWallpaper {
            monitors: monitors.into_iter().map(unquote).collect(),
        },
        SubCmd::PreviousWallpaper { monitors } => IpcMessage::PreviousWallpaper {
            monitors: monitors.into_iter().map(unquote).collect(),
        },
        SubCmd::ReloadWallpaper { monitors } => IpcMessage::ReloadWallpaper {
            monitors: monitors.into_iter().map(unquote).collect(),
        },
        SubCmd::PauseWallpaper { monitors } => IpcMessage::PauseWallpaper {
            monitors: monitors.into_iter().map(unquote).collect(),
        },
        SubCmd::ResumeWallpaper { monitors } => IpcMessage::ResumeWallpaper {
            monitors: monitors.into_iter().map(unquote).collect(),
        },
        SubCmd::TogglePauseWallpaper { monitors } => IpcMessage::TogglePauseWallpaper {
            monitors: monitors.into_iter().map(unquote).collect(),
        },
        SubCmd::GetStatus { json, monitors } => {
            json_resp = json;
            IpcMessage::GetStatus {
                monitors: monitors.into_iter().map(unquote).collect(),
            }
        }
    };

    conn.write_all(&serde_json::to_vec(&msg).unwrap()).unwrap();
    let mut buf = String::new();
    conn.read_to_string(&mut buf).unwrap();
    let res: Result<IpcResponse, IpcError> =
        serde_json::from_str(&buf).expect("wpaperd to return a valid json");
    match res {
        Ok(resp) => match resp {
            IpcResponse::CurrentWallpaper { path } => println!("{}", path.to_string_lossy()),
            IpcResponse::AllWallpapers { entries: paths } => {
                if json_resp {
                    #[derive(Serialize)]
                    struct Item {
                        display: String,
                        path: PathBuf,
                    }
                    let val = paths
                        .into_iter()
                        .map(|(name, path)| Item {
                            display: name,
                            path,
                        })
                        .collect::<Vec<_>>();
                    println!(
                        "{}",
                        serde_json::to_string(&val).expect("json encoding to work")
                    );
                } else {
                    for (monitor, path) in paths {
                        println!("{monitor}: {}", path.to_string_lossy());
                    }
                }
            }
            IpcResponse::DisplaysStatus { entries } => {
                /// Clean up the duration for human readability
                /// remove the milliseconds and the leading 0s
                fn clean_duration(duration: Duration) -> Duration {
                    let duration = duration.as_secs();
                    Duration::from_secs(if duration < 60 {
                        duration
                    } else if duration < 60 * 60 {
                        // if the duration is in minutes, remove the seconds
                        duration - duration % 60
                        // duration is in hours, remove the minutes and seconds
                    } else {
                        duration - duration % (60 * 60)
                    })
                }
                if json_resp {
                    #[derive(Serialize)]
                    struct Item {
                        display: String,
                        status: String,
                        #[serde(rename = "duration_left", with = "humantime_serde")]
                        duration_left: Option<Duration>,
                    }
                    let val = entries
                        .into_iter()
                        .map(|(display, status, duration_left)| Item {
                            display,
                            status,
                            duration_left: duration_left.map(clean_duration),
                        })
                        .collect::<Vec<_>>();
                    println!(
                        "{}",
                        serde_json::to_string(&val).expect("json encoding to work")
                    );
                } else {
                    for (monitor, status, duration_left) in entries {
                        println!(
                            "{monitor}: {status}{}",
                            if let Some(d) = duration_left {
                                format!(" ({} left)", humantime::format_duration(clean_duration(d)))
                            } else {
                                "".to_string()
                            }
                        );
                    }
                }
            }
            IpcResponse::Ok => (),
        },
        Err(err) => match err {
            IpcError::MonitorNotFound { monitor } => {
                eprintln!("monitor {monitor} could not be found")
            }
            IpcError::DrawErrors(errors) => {
                for (monitor, err) in errors {
                    eprintln!("Wallpaper could not be drawn for monitor {monitor}: {err}")
                }
            }
        },
    }
}
