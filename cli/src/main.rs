use std::{
    io::{Read, Write},
    os::unix::net::UnixStream,
};

use clap::Parser;
use wpaperd_ipc::{socket_path, IpcError, IpcMessage, IpcResponse};

/// Simple program to greet a person
#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[clap(subcommand)]
    subcmd: SubCmd,
}

#[derive(clap::Subcommand)]
enum SubCmd {
    GetWallpaper { monitor: String },
    AllWallpapers,
    NextWallpaper { monitors: Vec<String> },
    PreviousWallpaper { monitors: Vec<String> },
}

fn main() {
    let args = Args::parse();

    let mut conn = UnixStream::connect(&socket_path().unwrap()).unwrap();
    let msg = match args.subcmd {
        SubCmd::GetWallpaper { monitor } => IpcMessage::CurrentWallpaper { monitor },
        SubCmd::AllWallpapers => IpcMessage::AllWallpapers,
        SubCmd::NextWallpaper { monitors } => IpcMessage::NextWallpaper { monitors },
        SubCmd::PreviousWallpaper { monitors } => IpcMessage::PreviousWallpaper { monitors },
    };
    conn.write_all(&serde_json::to_vec(&msg).unwrap()).unwrap();
    // Add a new line after the message
    conn.write_all(b"\n").unwrap();
    let mut buf = String::new();
    conn.read_to_string(&mut buf).unwrap();
    let res: Result<IpcResponse, IpcError> = serde_json::from_str(&buf).unwrap();
    match res {
        Ok(resp) => match resp {
            IpcResponse::CurrentWallpaper { path } => println!("{path:?}"),
            IpcResponse::AllWallpapers { entries: paths } => {
                for (monitor, path) in paths {
                    println!("{monitor}: {path:?}");
                }
            }
            IpcResponse::Ok => (),
        },
        Err(err) => match err {
            IpcError::MonitorNotFound { monitor } => {
                eprintln!("monitor {monitor} could not be found")
            }
        },
    }
}
