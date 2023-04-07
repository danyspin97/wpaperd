# 0.3.0 (WIP)

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

