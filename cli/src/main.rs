mod opts;

use std::{
    io::{Read, Write},
    os::unix::net::UnixStream,
    path::PathBuf,
};

use clap::Parser;
use serde::Serialize;
use wpaperd_ipc::{socket_path, IpcError, IpcMessage, IpcResponse};

use crate::opts::{Opts, SubCmd};

fn main() {
    let args = Opts::parse();

    let mut json_resp = false;

    let mut conn = UnixStream::connect(socket_path().unwrap()).unwrap();
    let msg: IpcMessage = match args.subcmd {
        SubCmd::GetWallpaper { monitor } => IpcMessage::CurrentWallpaper { monitor },
        SubCmd::AllWallpapers { json } => {
            json_resp = json;
            IpcMessage::AllWallpapers
        }
        SubCmd::NextWallpaper { monitors } => IpcMessage::NextWallpaper { monitors },
        SubCmd::PreviousWallpaper { monitors } => IpcMessage::PreviousWallpaper { monitors },
        SubCmd::ReloadWallpaper { monitors } => IpcMessage::ReloadWallpaper { monitors },
        SubCmd::PauseWallpaper { monitors } => IpcMessage::PauseWallpaper { monitors },
        SubCmd::ResumeWallpaper { monitors } => IpcMessage::ResumeWallpaper { monitors },
        SubCmd::SetWallaper {
            wallpaper,
            monitors,
        } => IpcMessage::SetWallpaper {
            wallpaper,
            monitors,
        },
    };

    conn.write_all(&serde_json::to_vec(&msg).unwrap()).unwrap();
    let mut buf = String::new();
    conn.read_to_string(&mut buf).unwrap();
    let res: Result<IpcResponse, IpcError> = serde_json::from_str(&buf)
        .unwrap_or_else(|_| panic!("wpaperd to should valid json {}", &buf));

    match res {
        Ok(resp) => match resp {
            IpcResponse::CurrentWallpaper { path } => println!("{}", path.display()),
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
                        println!("{monitor}: {}", path.display());
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
