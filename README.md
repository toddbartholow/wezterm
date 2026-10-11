# Wez's Terminal

<img height="128" alt="WezTerm Icon" src="https://raw.githubusercontent.com/wezterm/wezterm/main/assets/icon/wezterm-icon.svg" align="left"> *A GPU-accelerated cross-platform terminal emulator and multiplexer written by <a href="https://github.com/wez">@wez</a> and implemented in <a href="https://www.rust-lang.org/">Rust</a>*

User facing docs and guide at: https://wezterm.org/

![Screenshot](docs/screenshots/two.png)

*Screenshot of wezterm on macOS, running vim*

## Changes in this fork

This is a personal fork of [WezTerm](https://github.com/wezterm/wezterm) with these changes:

* Rename the active tab with the `RenameTab` key assignment, the "Rename tab" command palette entry, or by double-clicking the tab.
* Reorder tabs by dragging them in the tab bar, with a move cursor and the dragged tab drawn in reverse video.
* Click a file path such as `src/main.rs:12:5` to open it at that line and column in the editor set by `file_link_editor`.
* Tabs whose program rang the bell or posted a notification while unfocused get a yellow dot, and Lua can read this as `needs_attention`.
* Spawned programs get `FORCE_HYPERLINK=1`, so CLIs that check for hyperlink support print clickable links.
* Title spinners from agent CLIs no longer pin the GUI thread, because tab bar rebuilds and `update-status` events are rate limited.
* On Windows, looking up a pane's working directory or foreground process takes about 1.5ms instead of 28-39ms.
* The bundled ConPTY on Windows is updated to 1.24.260710001.

## Installation

Releases from wezterm.org don't include the changes in this fork, so build it
from source by following [docs/install/source.md](docs/install/source.md),
which comes down to `cargo build --release` once the dependencies are in place.

## Getting help

Report problems with this fork in its
[issue tracker](https://github.com/toddbartholow/wezterm/issues), not upstream's.
For anything about WezTerm itself, see the documentation at https://wezterm.org/.

## Supporting WezTerm

WezTerm is written and maintained by Wez Furlong. If you use and like it,
please consider sponsoring Wez's work.

[Read more about sponsoring](https://wezterm.org/sponsor.html).

* [![Sponsor WezTerm](https://img.shields.io/github/sponsors/wez?label=Sponsor%20WezTerm&logo=github&style=for-the-badge)](https://github.com/sponsors/wez)
* [Patreon](https://patreon.com/WezFurlong)
* [Ko-Fi](https://ko-fi.com/wezfurlong)
* [Liberapay](https://liberapay.com/wez)
