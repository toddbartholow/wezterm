---
tags:
  - spawn
---
# `set_environment_variables`

Specifies a map of environment variables that should be set when spawning new
commands in the `"local"` domain.  This configuration is consulted at the time
that a program is launched.  It is not possible to update the environment of a
running program on any Operating System.

This is not used when working with remote domains.

wezterm sets `FORCE_HYPERLINK=1` in the environment of programs it launches,
so that tools which detect hyperlink support (such as many Node.js CLIs) emit
clickable OSC 8 links. To opt out, set it to `0`:

```lua
config.set_environment_variables = {
  FORCE_HYPERLINK = '0',
}
```

A `FORCE_HYPERLINK` value already present in wezterm's own environment is left
as is.

See also: [Launching Programs](../../launch.md#passing-environment-variables-to-the-spawned-program)
