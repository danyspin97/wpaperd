# wpaperd

![GitHub's release (latest by date)](https://img.shields.io/github/v/release/danyspin97/wpaperd?logo=github&style=flat-square)
[![GitHub license](https://img.shields.io/github/license/danyspin97/wpaperd?logo=github&style=flat-square)](https://github.com/danyspin97/wpaperd/blob/main/LICENSE.md)
![GitHub Workflow Status](https://img.shields.io/github/actions/workflow/status/danyspin97/wpaperd/cargo.yml?branch=main&logo=github&style=flat-square)

**wpaperd** is the modern wallpaper daemon for Wayland. It dynamically changes the current wallpaper,
either after a certain amount of time or via a command-line interface. It uses OpenGL ES to render
the images and have beautiful hardware-accelerated transitions, while being easy on resources.

*Notice*: wpaperd uses [wlr_layer_shell](https://wayland.app/protocols/wlr-layer-shell-unstable-v1)
wayland protocol, which is available on all wlroots based compositors (sway,
hyprland, ...) and on KDE. **Therefore it won't work on GNOME.**

## Features

- Different wallpaper for each display
- Pick a wallpaper from a directory
- Change the wallpaper after a set time
- Multiple sorting methods (random or ordered)
- Flexible TOML configuration file
- Hot config reloading for all settings
- Easy to use command line interface
- Hardware-accelerated configurable transitions
- Multiple background modes (center, fit, fill)
- Easy on resources (low CPU and memory usage)

## Getting started

### Dependencies

*wpaperd* is written in Rust and requires a working Cargo installation. It also depends on:

- `mesa`
- `wayland-client`
- `wayland-egl`
- `rinstall` (optional, for installing `wpaperd`)
- `libdav1d` (optional, for loading `avif` images)

### Build

To install `wpaperd`, clone the repository and build the project:

```bash
$ git clone https://github.com/danyspin97/wpaperd
$ cd wpaperd
$ cargo build --release
```

Generate the man pages by running `scdoc`:

```bash
$ scdoc < man/wpaperd-output.5.scd > man/wpaperd-output.5
```

### Install

You can install both the daemon (`wpaperd`) and cli (`wpaperctl`) using **rinstall**:
```bash
$ rinstall --yes
```

To run _wpaperd_, run the **daemon**:

```bash
$ wpaperd
```

If you want to automatically run it at startup, add this line to your sway configuration
(located in `$HOME/.config/sway/config`):

```
# Assuming it has been installed in ~/.local/bin/wpaperd
exec ~/.local/bin/wpaperd -d
```

Or in Hyprland:

```
exec-once=~/.local/bin/wpaperd -d
```

## Image formats support

wpaperd uses the [image] create to load and display images. Have a look on its
[documentation](https://github.com/image-rs/image/blob/main/README.md#supported-image-formats)
for the supported formats.

*Note*: To enable `avif` format, build wpaperd with `avif` feature (requires `libdav1d` to be
installed.

## Cycling images

When `path` is set to a directory, you can cycle the images by running the commands `next` and
`previous` using _wpaperctl_:

```bash
$ wpaperctl next
$ wpaperctl previous
```

When `sorting` is set to `ascending` and `descending`, _wpaperd_ will use the wallpaper name to
calculate the next wallpaper accordingly. When `sorting` is set to `random`, it will store
all the wallpapers shown in a queue, so that the commands `next` and `previous` can work
as intended.

The cycling of images can also be paused/resumed by running the `pause` and `resume` commands, or just `toggle-pause`, using _wpaperctl_:

```bash
$ wpaperctl pause
$ wpaperctl resume
$ wpaperctl toggle-pause
```

## Wallpaper Configuration

The configuration file for *wpaperd* is located in `XDG_CONFIG_HOME/wpaperd/config.toml`
(which defaults to `~/.config/wpaperd/config.toml`). Each section
represents a different display and can contain the following keys:

- `path`, path to the image to use as wallpaper or to a directory to pick the wallpaper from
- `duration`, how much time the image should be displayed until it is changed with a new one.
  It supports a human format for declaring the duration (e.g. `30s` or `10m`), described
  [here](https://docs.rs/humantime/latest/humantime/fn.parse_duration.html).
  This is only valid when path points to a directory. (_Optional_)
- `sorting`, choose the sorting order. Valid options are `ascending`, `descending`, and `random`,
  with the default being `random`. This is only valid when path points to a directory. (_Optional_)
- `group`, assign multiple displays to same group to share the same wallpaper when using
  `random` sorting; group must be a number. (_Optional_)
- `mode`, choose how to display the wallpaper when the size is different than the display
  resolution:
  - `fit` shows the entire image with black corners covering the empty space left
  - `fit-border-color` works like `fit`, but fill the empty space with the color of the border
    of the image; suggested for images that have a solid color in their border
  - `center` centers the image on the screen, leaving out the corners of the image that couldn't fit
  - `stretch` shows the entire image stretching it to fit the entire screen without leaving any
    black corner, changing the aspect ratio
  - `tile` shows the image multiple times horizontally and vertically to fill the screen
- `transition-time`, how many milliseconds should the transition run. (_Optional_, `300` by default).
- `offset`, offset the image on the screen, with a value from `0.0` to `1.0`. (_Optional_, `0.0` by
  default for `tile` mode and `0.5` for all the other modes)
- `queue-size`, decide how big the queue should be when `path` is set a directory and `sorting` is
   set to `random`. (_Optional_, `10` by default)
- `initial-transition`, enable the initial transition at wpaperd startup. (_Optional_, true by default)

The section `default` will be used as base for the all the display configuration; the section
`any` will be used for all the displays that are not explictly listed. This allows to have a
flexible configuration without repeating any settings. _wpaperd_ will check the configuration at
startup and each time it changes and provide help when it is incorrect.

This is the simplest configuration:

```toml
[DP-3]
path = "/home/danyspin97/github_octupus.png"

[DP-4]
path = "/home/danyspin97/Wallpapers"
duration = "30m"
```

This is a more complex configuration:

```toml
[default]
duration = "30m"
mode = "center"
sorting = "ascending"

[any]
path = "/home/danyspin97/default_wallpaper.png"

[DP-3]
path = "/home/danyspin97/Wallpapers"
```

If you're running sway, you can look for the available outputs and their ID by running:

```bash
$ swaymsg -t get_outputs
```

On Hyprland you can run:

```bash
$ hyprctl monitors
```

## FAQ

- The wallpapers are **slow to load**:
  wpaperd uses the `image` crate to load and decode the image. However, when built in `debug` mode
  the loading and decoding time takes from half a second to a couple, even on modern hardware.
  Try building wpaperd in release mode:

```bash
$ cargo build --release
```

## License

**wpaperd** is licensed under the [GPL-3.0+](/LICENSE.md) license.

[swaybg]: https://github.com/swaywm/swaybg
