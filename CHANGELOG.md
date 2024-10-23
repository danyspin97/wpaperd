# 1.1

## Breaking changes
- Rename `transition_time` and `queue_size` to kebab case (`transition-time and `queue-size`).

## New features
- Add `avif` feature to load `avif` images (requires `dav1d` library)
- Add `offset` configuration to move the wallpaper from its center
- Add `fit-border-color` background mode
- Add `initial-transition` configuration to disable the startup transition if needed
- Add `group` configuration to share the same wallpaper between multiple displays
- Match displays using their name or their description (fixes #90)
- Add multiple transition styles from [gl-transition]

[gl-transition]: https://gl-transitions.com/

# 1.0.1

- Fix drawing at start time

# 1.0.0

wpaperd is polished enough to call it 1.0.0

## Breaking Changes
- Version 0.3.0 had 2 different configuration files, one for wpaperd and one for the wallpapers.
  Remove the former and move the latter (`wallpaper.toml`) to `config.toml`
- wpaperd `--no-daemon` behavior is now the default, `--daemon`/`-d` option have been
  added to fork and live in the background

## Other Changes

- Use openGL ES to render the wallpaper instead of a Wayland memory buffer
- Add transitions when switching to a wallpaper or to the other
  * Add `transition-time` to control the duratoin of the transition
- Add `wpaperctl` command line program to control wpaperd
  * Let wpaperd switch the next and previous wallpaper
  * Get the current wallpaper for each displays
  * Reload the current wallpaper
- Add `--notify` flag to wpaperd for readiness
- Add `sorting` option to allow wpaperd to pick wallpaper in an ordered manner
- Improve error checking and messages
- Improve config parsing and checking
- Remove --use-scaled-window option
- Implement a filelist cache to avoid reading from disk every time
- Add a `mode` to choose how to display the wallpaper (`center`, `fit`, `stretch` or `tile`)
- Add a `any` section to configuration file to allow for more flexible configurations
- Update MSRV to 1.61.0
- Use a black pixel as starting image

# 0.3.0

- Replace timer library with calloop::sources::Timer (fixes #13)
- Refactor wpaperd to use wayland-rs 0.30 and latest smithay-client-toolkit
  (fixes #14)
- Do not crash when a new display is added or removed
- The wallpaper duration setting is now more reliable
- Fix panic when a directory is empty (fixes #27)
- Remove `output-config` from config
- Remove `--config` argument from cli
- Rename wpaperd.conf to wpaperd.toml and output.conf to wallpaper.toml
  (fixes #25)
- Cleanup code

# 0.2.0

- Use rust implementation of wayland-client library
- Draw to native resolution by default, add --use-scaled-window to
  match old behavior. Previously the wallpaper was drawn to the scaled
  resolution for the output selected and the compositor was doing the scaling.
- Add apply-shadow option

# 0.1.1

- Add wpaperd.1 manpage and completions
- Log output to a file
- Don't redraw when the dimensions are the same
- Add wrap_help feature to clap

# 0.1.0

Initial release!

