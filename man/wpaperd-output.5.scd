wpaperd-output(5)

# NAME

wpaperd-output - format used by wpaperd to define the various outputs

# DESCRIPTION

The wpaperd output format contains the information about each input and the corresponding wallpaper.
This data is stored in XDG_CONFIG_HOME/wpaperd/output.conf (which defaults to
$HOME/.config/wpaperd/output.conf).

# SYNTAX

This file is a simple TOML configuration divided in sections, one for each output (or screen).

## SECTION

Each section is named after the output identifier. If you are using _sway_, you can lookup
the output identifier by calling:

```
$ swaymsg -t get_outputs
```

The valid keys for the section are the following:

- *path*, path to the image/directory
- *duration*, how much time the image should be displayed until it is changed with a new one.
  This is only valid when path points to a directory. (_Optional_)
- *sorting*, choose the sorting order. Valid options are `ascending`, `descending`, and `random`,
  with the unspecified or default being `random` (_Optional_)

## DEFAULT SECTION

A special section is the one called "*Default*", that will be used as fallback for all the
outputs not listed in this configuration file.

# EXAMPLE

```
[default]
path = "/home/danyspin97/Pictures/Wallpapers/"
duration = "30m"
sorting = "ascending"

[eDP-1]
path = "/home/danyspin97/Pictures/Wallpapers/github_octupus.png"
apply-shadow = true

[DP-2]
path = "/home/danyspin97/Pictures/Landscapes/"
sorting = "descending"
```

# AUTHOR
Maintained by Danilo Spinella <danilo.spinella@suse.com>.
