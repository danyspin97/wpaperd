//! IPC socket server.
//! Based on <https://github.com/catacombing/catacomb/blob/master/src/ipc_server.rs>

use std::collections::HashSet;
use std::fs;
use std::io::{BufReader, BufWriter, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};

use color_eyre::eyre::{ensure, WrapErr};
use color_eyre::{Result, Section};
use smithay_client_toolkit::reexports::client::QueueHandle;
use wpaperd_ipc::{IpcError, IpcMessage, IpcResponse};

use crate::socket::SocketSource;
use crate::surface::Surface;
use crate::Wpaperd;

/// Create an IPC socket.
pub fn listen_on_ipc_socket(socket_path: &Path) -> Result<SocketSource> {
    // Try to delete the socket if it exists already.
    if socket_path.exists() {
        fs::remove_file(socket_path)
            .wrap_err_with(|| format!("Failed to remove file {socket_path:?}"))?;
    }

    // Spawn unix socket event source.
    let listener = UnixListener::bind(socket_path)
        .wrap_err_with(|| format!("Failed to bind socket {socket_path:?}"))?;
    let socket = SocketSource::new(listener)
        .wrap_err_with(|| format!("Failed to create a socket in {socket_path:?}"))?;
    Ok(socket)
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
pub fn handle_message(
    ustream: UnixStream,
    qh: QueueHandle<Wpaperd>,
    wpaperd: &mut Wpaperd,
) -> Result<()> {
    const SIZE: usize = 4096;
    let mut buffer = [0; SIZE];

    // Read new content to buffer.
    let mut stream = BufReader::new(&ustream);
    let n = stream
        .read(&mut buffer)
        .wrap_err("Failed to read data from the IPC stream")?;
    // The message is empty
    if n == 0 {
        return Ok(());
    }
    ensure!(
        n != SIZE,
        "The message received was bigger than the buffer size"
    );

    // Read pending events on socket.
    let message: IpcMessage = serde_json::from_slice(&buffer[..n])
        .wrap_err_with(|| format!("Failed to deserialize message {:?}", &buffer[..n]))?;

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
                    surface.load_new_wallpaper();
                }

                IpcResponse::Ok
            })
        }

        IpcMessage::NextWallpaper { monitors } => check_monitors(wpaperd, &monitors).map(|_| {
            for surface in collect_surfaces(wpaperd, monitors) {
                surface.image_picker.next_image(
                    &surface.wallpaper_info.path,
                    &surface.wallpaper_info.recursive,
                );
                surface.load_new_wallpaper();
            }

            IpcResponse::Ok
        }),

        IpcMessage::ReloadWallpaper { monitors } => check_monitors(wpaperd, &monitors).map(|_| {
            for surface in collect_surfaces(wpaperd, monitors) {
                surface.image_picker.reload();
                surface.load_new_wallpaper();
                surface.queue_draw(&qh);
            }

            IpcResponse::Ok
        }),

        IpcMessage::PauseWallpaper { monitors } => check_monitors(wpaperd, &monitors).map(|_| {
            for surface in collect_surfaces(wpaperd, monitors) {
                surface.pause();
            }
            IpcResponse::Ok
        }),

        IpcMessage::ResumeWallpaper { monitors } => check_monitors(wpaperd, &monitors).map(|_| {
            for surface in collect_surfaces(wpaperd, monitors) {
                surface.resume();
            }
            IpcResponse::Ok
        }),

        IpcMessage::TogglePauseWallpaper { monitors } => {
            check_monitors(wpaperd, &monitors).map(|_| {
                for surface in collect_surfaces(wpaperd, monitors) {
                    surface.toggle_pause();
                }

                IpcResponse::Ok
            })
        }

        IpcMessage::GetStatus { monitors } => {
            check_monitors(wpaperd, &monitors).map(|_| IpcResponse::DisplaysStatus {
                entries: collect_surfaces(wpaperd, monitors)
                    .iter()
                    .map(|surface| {
                        (
                            surface.name().to_string(),
                            surface.status().to_string(),
                            surface.get_remaining_duration(),
                        )
                    })
                    .collect(),
            })
        }

        IpcMessage::SetWallpaper { path, monitors } => {
            if !path.exists() {
                Err(IpcError::DrawErrors(vec![(
                    String::new(),
                    format!("File not found: {}", path.display()),
                )]))
            } else if !path.is_file() {
                Err(IpcError::DrawErrors(vec![(
                    String::new(),
                    format!("Path is not a file: {}", path.display()),
                )]))
            } else {
                check_monitors(wpaperd, &monitors).map(|_| {
                    for surface in collect_surfaces(wpaperd, monitors) {
                        surface.image_picker.set_image(path.clone());
                        surface.pause();
                        surface.load_new_wallpaper();
                    }
                    IpcResponse::Ok
                })
            }
        }
    };

    let mut stream = BufWriter::new(ustream);
    stream
        .write_all(&serde_json::to_vec(&resp).unwrap())
        .wrap_err("Failed to write response to the IPC client")
        .suggestion("The client might have died, try running it again")?;

    Ok(())
}
