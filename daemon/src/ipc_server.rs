//! IPC socket server.
//! Based on https://github.com/catacombing/catacomb/blob/master/src/ipc_server.rs

use std::collections::HashSet;
use std::fs;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};

use color_eyre::eyre::Context;
use color_eyre::Result;
use log::error;
use smithay_client_toolkit::reexports::calloop::LoopHandle;
use wpaperd_ipc::{IpcError, IpcMessage, IpcResponse};

use crate::socket::SocketSource;
use crate::surface::Surface;
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

fn check_monitors(wpaperd: &Wpaperd, monitors: &Vec<String>) -> Result<(), IpcError> {
    for monitor in monitors {
        if !wpaperd
            .surfaces
            .iter()
            .any(|surface| surface.name() == monitor)
        {
            return Err(IpcError::MonitorNotFound {
                monitor: monitor.to_owned(),
            });
        }
    }

    Ok(())
}

fn collect_surfaces(wpaperd: &mut Wpaperd, monitors: Vec<String>) -> Vec<&mut Surface> {
    let monitors: HashSet<String> = HashSet::from_iter(monitors);
    if monitors.is_empty() {
        return wpaperd.surfaces.iter_mut().collect();
    };

    wpaperd
        .surfaces
        .iter_mut()
        .filter(|surface| monitors.contains(surface.name()))
        .collect()
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
            .map(|surface| surface.image_picker.current_image())
            .map(|path| IpcResponse::CurrentWallpaper { path })
            .ok_or(IpcError::MonitorNotFound { monitor }),
        IpcMessage::AllWallpapers => Ok(IpcResponse::AllWallpapers {
            entries: wpaperd
                .surfaces
                .iter()
                .map(|surface| {
                    (
                        surface.name().to_string(),
                        surface.image_picker.current_image(),
                    )
                })
                .collect::<Vec<(String, PathBuf)>>(),
        }),

        IpcMessage::PreviousWallpaper { monitors } => {
            check_monitors(wpaperd, &monitors).map(|_| {
                for surface in collect_surfaces(wpaperd, monitors) {
                    surface.image_picker.previous_image();
                }

                IpcResponse::Ok
            })
        }

        IpcMessage::NextWallpaper { monitors } => check_monitors(wpaperd, &monitors).map(|_| {
            for surface in collect_surfaces(wpaperd, monitors) {
                surface.image_picker.next_image();
            }

            IpcResponse::Ok
        }),

        IpcMessage::ReloadConfig => {
            wpaperd.reload_config()?;
            Ok(IpcResponse::Ok)
        }
    };

    let mut stream = BufWriter::new(ustream);
    stream
        .write_all(&serde_json::to_vec(&resp).unwrap())
        .unwrap();

    Ok(())
}
