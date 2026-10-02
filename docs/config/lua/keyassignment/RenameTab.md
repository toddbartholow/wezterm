# `RenameTab`

{{since('nightly')}}

Prompts for a new title for the active tab. The prompt starts with the title
the tab shows now, so you can edit it rather than retype it.

* `Enter` applies the new title
* `Esc` cancels and leaves the title as it was
* Submitting an empty title clears the tab's title, so that the tab shows the
  title set by the program running in it again

A title set this way takes priority over the title the program running in the
tab sets for itself, which is useful for programs that keep changing their
title.

`RenameTab` is also available as "Rename tab" in the
[command palette](ActivateCommandPalette.md), and double-clicking a tab in the
tab bar does the same thing. It has no default key binding; to add one:

```lua
config.keys = {
  { key = 'R', mods = 'CTRL|SHIFT|ALT', action = wezterm.action.RenameTab },
}
```

To set a tab title from a script instead, see
[MuxTab:set_title](../MuxTab/set_title.md) and
[wezterm cli set-tab-title](../../../cli/cli/set-tab-title.md).
