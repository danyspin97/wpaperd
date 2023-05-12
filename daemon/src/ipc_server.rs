//! IPC socket server.
//! Based on https://github.com/catacombing/catacomb/blob/master/src/ipc_server.rs

use std::fs;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};

use color_eyre::eyre::Context;
use color_eyre::Result;
use log::error;
use serde::{Deserialize, Serialize};
use smithay_client_toolkit::reexports::calloop::LoopHandle;

use crate::socket::SocketSource;
use crate::Wpaperd;

/// Create an IPC socket.
pub fn spawn_ipc_socket(event_loop: &LoopHandle<Wpaperd>, socket_path: &Path) -> Result<()> {
    // Try to delete the socket if it exists already.
    if socket_path.exists() {
        fs::remove_file(socket_path)?;
    }

    // Spawn unix socket event source.
    let listener = UnixListener::bind(socket_path)?;
    let socket = SocketSource::new(listener)?;

    // Add source to calloop loop.
    let mut message_buffer = String::new();
    event_loop.insert_source(socket, move |stream, _, wpaperd| {
        if let Err(err) = handle_message(&mut message_buffer, stream, wpaperd) {
            error!("{}", err);
        }
    })?;

    Ok(())
}

#[derive(Deserialize)]
enum IpcMessage {
    CurrentWallpaper { monitor: String },
    CurrentWallpapers,
}

#[derive(Serialize)]
enum IpcResponse {
    CurrentWallpaper { path: PathBuf },
    CurrentWallpapers { paths: Vec<(String, PathBuf)> },
}

#[derive(Serialize)]
enum IpcError {
    MonitorNotFound,
}

/// Handle IPC socket messages.
fn handle_message(buffer: &mut String, ustream: UnixStream, wpaperd: &mut Wpaperd) -> Result<()> {
    buffer.clear();

    // Read new content to buffer.
    let mut stream = BufReader::new(&ustream);
    let n = stream
        .read_line(buffer)
        .context("error while reading line from IPC")?;
    // The message is empty
    if n == 0 {
        return Ok(());
    }

    // Read pending events on socket.
    let message: IpcMessage = serde_json::from_str(buffer)
        .with_context(|| format!("error while deserializing message {buffer:?}"))?;

    // Handle IPC events.
    let resp: Result<IpcResponse, IpcError> = match message {
        IpcMessage::CurrentWallpaper { monitor } => wpaperd
            .surfaces
            .iter()
            .find(|surface| surface.name() == monitor)
            .map(|surface| surface.current_img.clone())
            .map(|path| IpcResponse::CurrentWallpaper { path })
            .ok_or(IpcError::MonitorNotFound),
        IpcMessage::CurrentWallpapers => Ok(IpcResponse::CurrentWallpapers {
            paths: wpaperd
                .surfaces
                .iter()
                .map(|surface| (surface.name().to_string(), surface.current_img.clone()))
                .collect::<Vec<(String, PathBuf)>>(),
        }),
    };

    let mut stream = BufWriter::new(ustream);
    stream
        .write_all(&serde_json::to_vec(&resp).unwrap())
        .unwrap();

    Ok(())
}
