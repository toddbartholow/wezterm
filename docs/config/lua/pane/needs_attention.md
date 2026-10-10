# `pane:needs_attention()`

{{since('nightly')}}

Returns true if the program in the pane rang the bell, or posted a
notification with `OSC 9` or `OSC 777`, while the pane didn't have focus.
It becomes false again when the pane is focused.

Unlike [pane:has_unseen_output()](has_unseen_output.md), which is set by any
output, this is a signal that the program is waiting on the user. Coding
agents and long running commands commonly ring the bell or send a
notification when they finish or need a decision.

This is tracked for panes in the local domain. It is always false for panes
in remote multiplexer domains.

See also [PaneInformation.needs_attention](../PaneInformation.md) and
[TabInformation.needs_attention](../TabInformation.md).
