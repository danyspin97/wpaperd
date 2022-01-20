wpaper-output(5)

# NAME

wpaper-output - format used by wpaper to define the various outputs

# DESCRIPTION

The wpaper output format contains the information about each input and the corresponding wallpaper.
This data is stored in XDG_CONFIG_HOME/wpaper/output.conf (which defaults to
$HOME/.config/wpaper/output.conf).

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

## DEFAULT SECTION

A special section is the one called "*Default*", that will be used as fallback for all the
outputs not listed in this configuration file.

# EXAMPLE

```
[default]
path = "/home/danyspin97/Pictures/Wallpapers/"
duration = "30m"

[eDP-1]
path = "/home/danyspin97/Pictures/Wallpapers/github_octupus.png"
```

# AUTHOR
Maintained by Danilo Spinella <danilo.spinella@suse.com>.