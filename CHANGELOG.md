# 0.2.0 (unreleased)

- Use rust implementation of wayland-client library
- Draw to native resolution by default, add --use-scaled-window to
  match old behavior. Previously the wallpaper was drawn to the scaled
  resolution for the output selected and the compositor was doing the scaling.

# 0.1.1

- Add wpaperd.1 manpage and completions
- Log output to a file
- Don't redraw when the dimensions are the same
- Add wrap_help feature to clap

# 0.1.0

Initial release!

