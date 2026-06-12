# 1.3.0
### 🚀 Features

- Add wpaperctl set command
- Preserve explicit pause across set operations
- Validate image format before accepting set
- *(image_picker)* Simplify functions
- *(image_picker)* Add a queue of actions
- *(egl_context)* Use the correct size when recreating the context

### 🐛 Bug Fixes

- Handle monitor descriptions with more than one space before the port information
- *(image_picker)* Remove legacy code, fixing random sorting
- Update `Cargo.lock`
- *(daemon/image_loader)* Apply image orientation
- Include output description in config error
- Handle wallpaper link to missing image file
- *(nvidia)* Resolve EGL driver bugs by using wl_egl_window_resize
- Correctly handle multiple monitors
- Resource leak
- Add EGL_RENDERABLE_TYPE/EGL_OPENGL_ES2_BIT to config
- *(egl_context)* Fix resizing on Intel and AMD graphic cards
- *(image_picker)* Fix queue handling when there are few images
- *(wpaperd)* Mark wpaperd surface as input
- *(config)* Respect default recursive value
- *(transitions)* They work now
- *(image_picker)* Fix next_image going forward 2 times
- *(image_picker)* Fix tests compilation issue

### 🚜 Refactor

- Replace tuple with ImageResult enum

### 📚 Documentation

- Put Dell example in double quotes
- Add set command to README
- *(README)* Improve FAQ and add hyprland support notice

### ⚙️ Miscellaneous Tasks

- Update checkout action to v4
- Avoid using deprecated actions
- Update `upload-sarif` action
- Only run release workflow on dispatch - avoid "no jobs were run" error
- Avoid using deprecated `audit-check`
- *(fmt)* Wups. Forgot to `cargo fmt`.
- Update dependencies
- Remove unused import
- *(flake)* Update inputs
- *(flake)* Pull in Nixpkgs' own jemalloc crate
- *(surface)* Document state machine
- Set version to 1.3.0
- Update dependencies

# 1.2.2
## Breaking changes
- `initial-transition` is now set to false by default
## Bugfixes
- Fix leak by reusing memory for loading wallpapers (fixes #131).
- Fix binding previous wallpaper to properly show transitions.
- Disable vsync to let the event loop handle the transitions.
- Do not create empty config directory (`$XDG_CONFIG_HOME/wpaperd`) (#132).

# 1.2.1
## Bux fixes
- Load `exec` value from default (fixes #123).

# 1.2.0
## New features
- Add a new `recursive` config option, enabled by default (fixes #112).
- Add support for regex (thanks to [@ein-shived](https://github.com/ein-shved), #108)
- Add support for running scripts every time the wallpaper change (thanks to [@Primitheus](https://github.com/Primitheus), #121)
## Bug fixes
- Redraw wallpaper when the screen rotates
- Skip the transition when it's happening in background
  (i.e. the wallpaper is not currently focused).

# 1.1.1
## New features
- Make jemalloc feature optional, enabled by default

## Bug fixes
- Fix build on non x86_64 architectures

# 1.1.0

## Breaking changes
- Rename `transition_time` and `queue_size` to kebab case (`transition-time` and `queue-size`).

## New features
- Add `avif` feature to load `avif` images (requires `dav1d` library)
- Add `offset` configuration to move the wallpaper from the center of the screen
- Add `fit-border-color` background mode, which works like `fit` but uses the color of the
  border of the image to fill the rest of the screen not covered
- Add `initial-transition` configuration to disable the startup transition if needed
- Add `group` configuration to share the same wallpaper between multiple displays
- Match displays using their name or their description (fixes #90)
- Add multiple transition styles from [gl-transition]
- Add a link to the current wallpaper in `.local/state/wpaperd/wallpapers` for each display
- Listen to SIGINT, SIGTERM and SIGHUP signals and do a graceful exit

## Other changes
- Reworked the timer handling
- Reworked wallpaper loading to be lighter
- Many bug fixes and small changes on its behavior

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

