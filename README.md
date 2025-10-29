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
$ rinstall install --yes
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

**Notice**: _the queue only works when `queue-size` setting (which defaults to `10`) is bigger
than the number of available images in the folder_.

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
- `recursive`, recursively iterate the directory `path` when looking for available wallpapers;
  it is only valid when `path` points to a directory. (_Optional_, true by default)
- `exec`, path to a script that will be executed every time the wallpaper changes; the script
  will be called with the display and the new wallpaper as argument. (_Optional_)

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

["Dell Inc. DELL U2419H GY1VSS2"]
path = "/home/danyspin97/Wallpapers/1080p"
```

Another way to match a section to one or more displays is by using regex:

```toml
# Matches all displays that have the string "LG" in their description
["re:LG"]

# Matches all displays that are connected through display port (e.g. DP-1, DP-2)
["re:DP-\\d"]
```

If you're running sway, you can look for the available outputs and
their ID (or description) by running:

```bash
$ swaymsg -t get_outputs
```

On Hyprland you can run:

```bash
$ hyprctl monitors
```

Output descriptions take priority over output IDs.

### Wallpaper link

**wpaperd** creates a symlink in `XDG_STATE_HOME/wpaperd/wallpapers`
(`.local/state/wpaperd/wallpapers` by default) for each display that points to the current
wallpaper used. This is useful to integrate the current status with other components.

```bash
~ $ ls ~/.local/state/wpaperd/wallpapers
DP-3@      DP-4@
```

### Exec script

With the `exec` config parameter, wpaperd will execute a script every time the wallpaper changes.
This is an example script that can be used with pywal:

```bash
#!/bin/bash

display=$1
wallpaper=$2

echo "Display is : $display"
echo "Wallpaper path is: $wallpaper"

# Update Pywal
echo ":: Applying pywal with $wallpaper"
wal -q -i "$wallpaper"
source "$HOME/.cache/wal/colors.sh"
```

### Transitions

Since version `1.1`, wpaperd support multiple transitions types, taken from [gl-transition].
The transition can be changed at runtime and most of them are configurable, e.g. you can change
the direction of `directional` transition or the speed for `dissolve` transition, etc. Every
transition bring each own defaults, so you can just leave everything empty unless you want
to customize the transition. The default `transition-time` for each transition is different,
to provide a better experience out of the box.
To switch between available transitions, add the following to `wpaperd` configuration:

```toml
[default]
path = "/home/danyspin97/Wallpapers"
duration = "30m"
sorting = "ascending"

[default.transition.hexagonalize]
# default values for hexagonalize
# steps = 50
# horizontal-hexagons = 20.0
```

[gl-transition]: https://gl-transitions.com/

This is the list of available transitions with their own settings and defaults:

- `book-flip` (`2000`)
- [`bounce`](https://gl-transitions.com/editor/Bounce) (`4000`):
  + `shadow-colour`: `[0.0, 0.0, 0.0, 0.6]`
  + `shadow-height`: `0.075`
  - `bounces`: `3.0`
- [`bow-tie-horizontal`](https://gl-transitions.com/editor/BowTieHorizontal) (`1500`)
- [`bow-tie-vertical`](https://gl-transitions.com/editor/BowTieVertical) (`1500`)
- [`butterfly-wave-scrawler`](https://gl-transitions.com/editor/ButterflyWaveScrawler) (`2000`):
  + `amplitude`: `1.0`
  + `waves`: `30.0`
  + `color-separation`: `0.3`
- [`circle`](https://gl-transitions.com/editor/circle) (`3000`)
- [`circle-crop`](https://gl-transitions.com/editor/CircleCrop) (`3000`)
  + `bgcolor`: `[0.0, 0.0, 0.0, 1.0]`
- [`circle-open`](https://gl-transitions.com/editor/circleopen) (`1500`):
  + `smoothness`: `0.3`
  + `opening`: `true`
- [`colour-distance`](https://gl-transitions.com/editor/ColourDistance) (`2000`):
  + `power`: `5.0`
- [`cross-warp`](https://gl-transitions.com/editor/crosswarp) (`1000`):
- [`cross-zoom`](https://gl-transitions.com/editor/CrossZoom) (`2000`):
  + `strength`: `0.4`
- [`directional`](https://gl-transitions.com/editor/Directional) (`1000`):
  + `direction`: `[0.0, 1.0]`
- `directional-scaled` (`1000`):
  + `direction`: `[0.0, 1.0]`
  + `scale`: `0.7`
- [`directional-wipe`](https://gl-transitions.com/editor/directionalwipe) (`1000`):
  + `direction`: `[1.0, -1.0]`
  + `smoothness`: `0.5`
- `dissolve` (`1000`):
  + `line-width`: `0.1`
  + `spread-clr`: `[1.0, 0.0, 0.0]`
  + `hot-clr`: `[0.9, 0.9, 0.2]`
  + `intensity`: `1.0`
  + `pow`: `5.0`
- [`doom`](https://gl-transitions.com/editor/DoomScreenTransition) (`2000`):
  + `bars`: `30`
  + `amplitude`: `2.0`
  + `noise`: `0.1`
  + `frequency`: `0.5`
  + `drip-scale`: `0.5`
- [`doorway`](https://gl-transitions.com/editor/doorway) (`1500`):
  + `reflection`: `0.4`
  + `perspective`: `0.4`
  + `depth`: `3.0`
- [`dreamy`](https://gl-transitions.com/editor/Dreamy) (`1500`)
- [`dreamy-zoom`](https://gl-transitions.com/editor/DreamyZoom) (`1500`):
  + `rotation`: `6.0`
  + `scale`: `1.2`
- `edge` (`1500`):
  + `thickness`: `0.001`
  + `brightness`: `8.0`
- [`fade`](https://gl-transitions.com/editor/fade) (`300`)
- `film-burn` (`2000`):
  + `seed`: `2.31`
- [`glitch-displace`](https://gl-transitions.com/editor/GlitchDisplace) (`1500`)
- [`glitch-memories`](https://gl-transitions.com/editor/GlitchMemories) (`1500`)
- [`grid-flip`](https://gl-transitions.com/editor/GridFlip) (`1500`):
  + `size`: `[4, 4]`
  + `pause`: `0.1`
  + `divider-width`: `0.05`
  + `bgcolor`: `[0.0, 0.0, 0.0, 1.0]`
  + `randomness`: `0.1`
- [`hexagonalize`](https://gl-transitions.com/editor/hexagonalize) (`2000`):
  + `steps`: `50`
  + `horizontal-hexagons`: `20.0`
- `horizontal-close` (`2000`)
- `horizontal-open` (`2000`)
- [`inverted-page-curl`](https://gl-transitions.com/editor/InvertedPageCurl) (`2000`)
- `left-right` (`2000`)
- [`linear-blur`](https://gl-transitions.com/editor/LinearBlur) (`800`):
  + `intensity`: `0.1`
- `mosaic` (`2000`):
  + `endx`: `2`
  + `endy`: `-1`
- `overexposure` (`2000`)
- [`pixelize`](https://gl-transitions.com/editor/pixelize) (`1500`):
  + `squares-min`: `[20, 20]`
  + `steps`: `50`
- [`polkan-dots-curtain`](https://gl-transitions.com/editor/PolkaDotsCurtain) (`2000`):
  + `dots`: `20.0`
  + `center`: `[0.0, 0.0]`
- `radial` (`1500`):
  + `smoothness`: `1.0`
- `rectangle` (`2000`):
  + `bgcolor`: `[0.0, 0.0, 0.0, 1.0]`
- [`ripple`](https://gl-transitions.com/editor/ripple) (`1500`):
  + `amplitude`: `100.0`
  + `speed`: `50.0`
- `rolls` (`2000`):
  + `rolls-type`: `0`
  + `rot-down`: `false`
- [`rotate-scale-fade`](https://gl-transitions.com/editor/rotate_scale_fade) (`1500`):
  + `center`: `[0.0, 0.5]`
  + `rotations`: `1.0`
  + `scale`: `8.0`
  + `back-color`: `[0.15, 0.15, 0.15, 1.0]`
- `rotate-scale-vanish` (`1500`):
  + `fade-in-second`: `true`
  + `reverse-effect`: `false`
  + `reverse-rotation`: `false`
- [`simple-zoom`](https://gl-transitions.com/editor/SimpleZoom) (`1500`):
  + `zoom-quickness`: `0.8`
- `slides` (`1500`):
  + `slides-type`: `0`
  + `slides-in`: `false`
- `static-fade` (`1500`):
  + `n-noise-pixels`: `200.0`
  + `static-luminisotiy`: `0.8`
- [`stereo-viewer`](https://gl-transitions.com/editor/StereoViewer) (`2000`):
  + `zoom`: `0.88`
  + `corner_radius`: `0.22`
- [`swirl`](https://gl-transitions.com/editor/Swirl) (`1500`)
- `tv-static` (`1000`):
  + `offset`: `0.05`
- [`water-drop`](https://gl-transitions.com/editor/WaterDrop) (`1500`):
  + `amplitude`: `30.0`
  + `speed`: `30.0`
- [`window-blinds`](https://gl-transitions.com/editor/windowblinds) (`1500`)

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
