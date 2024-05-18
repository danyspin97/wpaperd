wpaperd-output(5)

# NAME

wpaperd-output - format used by wpaperd to define the various displays

# DESCRIPTION

The wpaperd configuration file contains the information about each display, the corresponding
wallpaper and its settings. This file is located in XDG_CONFIG_HOME/wpaperd/config.toml
(which defaults to $HOME/.config/wpaperd/config.toml).

# SYNTAX

This file is a simple TOML configuration divided in sections, one for each display.

## SECTION

Each section is named after the display identifier. If you are using _sway_, you can lookup
the output identifier by calling:

```
$ swaymsg -t get_outputs
```

On Hyprland, you can do it with:

```
$ hyprctl monitors
```

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
- `mode`, choose how to display the wallpaper when the size is different than the display
  resolution:
  - `fit` shows the entire image with black corners covering the empty space left
  - `center` centers the image on the screen, leaving out the corners of the image that couldn't fit
  - `stretch` shows the entire image stretching it to fit the entire screen without leaving any
    black corner, changing the aspect ratio
  - `tile` shows the image multiple times horizontally and vertically to fill the screen
- `transition_time`, how many milliseconds should the transition run. (_Optional_, `300` by default).
- `queue_size`, decide how big the queue should be when `path` is set a directory and `sorting` is
   set to `random`. (_Optional_, `10` by default)
- `initial_transition`, whether or not to transition from the initial black screen (_Optional_, `true` by default)
  

## DEFAULT SECTION

The section "*default*" will be used as base for the all the display configuration; the section
"*any*" will be used for all the displays that are not explictly listed. This allows to have a
flexible configuration without repeating any settings. _wpaperd_ will check the configuration at
startup and each time it changes and provide help when it is incorrect.

# EXAMPLE

The simplest configuration looks like this:

```
[DP-3]
path = "/home/danyspin97/github_octupus.png"

[DP-4]
path = "/home/danyspin97/Wallpapers"
duration = "30m"
```

A more complex configuration is also possible:

```
[default]
duration = "30m"
mode = "center"
sorting = "ascending"

[any]
path = "/home/danyspin97/default_wallpaper.png"

[DP-3]
path = "/home/danyspin97/Wallpapers"
```

# AUTHOR
Maintained by Danilo Spinella <danilo.spinella@suse.com>.
