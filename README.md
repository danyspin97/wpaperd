# wpaperd

**wpaperd** is a minimal wallpaper daemon for Wayland. It allows the user to choose a different
image for each output (aka for each monitor) just as *[swaybg]*. Moreover, a directory can be
chosen and *wpaperd* will randomly choose an image from it. Optionally, the user can set a
duration, after which the image displayed will be changed with another random one.

## Features

- Choose your wallpaper for each input
- Randomly choose an image from a directory
- Change the random image after a set duration
- Configurable via a TOML configuration file
- Reload config at runtime and apply new settings
- Written entirely in Rust, it has no system dependencies

## Getting started

### Build

To install `wpaperd`, clone the repository and build the project:

```bash
$ git clone https://github.com/danyspin97/wpaperd
$ cd wpaperd
$ cargo build --release
```

### Install

You can install wpaperd using cargo:

```bash
$ cargo install --path="."
```

Otherwise you can install it using rinstall:

```bash
# First generate the man page
$ scdoc < man/wpaperd-output.5.scd > man/wpaperd-output.5
$ rinstall -y
```

To run it, execute the `wpaperd` program:

```bash
$ wpaperd
```

If you want to automatically run it at startup, add this line to your sway configuration
(located in `$HOME/.config/sway/config`) depending on your installation method:

```
# For installation using cargo
exec ~/.cargo/bin/wpaperd
# For installation using rinstall
exec ~/.local/bin/wpaperd
```

## Output Configuration

The output configuration file for *wpaperd* is located in `XDG_CONFIG_HOME/wpaperd/output.conf`
(which defaults to `$HOME/.config/wpaperd/output.conf`) and is a TOML file. Each section
represents a different output and contains the following keys:

- `path`, path to the image/directory
- `duration`, how much time the image should be displayed until it is changed with a new one.
  This is only valid when path points to a directory. (_Optional_)

The section `default` will be used as fallback for the all the outputs that aren't listed in
the config file. This is an example configuration:

```toml
[default]
path = "/home/danyspin97/Pictures/Wallpapers/"
duration = "30m"

[eDP-1]
path = "/home/danyspin97/Pictures/Wallpapers/github_octupus.png"
```

If you're running sway, you can look for the available outputs and their ID by running:

```bash
$ swaymsg -t get_outputs
```

Every time you update the configuration while the program is running, the changes will
be applied automatically.

## License

**wpaperd** is licensed under the [GPL-3.0+](/LICENSE.md) license.

[swaybg]: https://github.com/swaywm/swaybg
