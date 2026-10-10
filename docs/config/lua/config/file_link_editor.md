---
tags:
  - hyperlink
---
# `file_link_editor`

Specifies the command used to open a link to a file when it is clicked.
This covers file paths that the default [hyperlink_rules](hyperlink_rules.md)
find in terminal output, such as `src/main.rs:12:5`, and `file://` links that
programs emit as [explicit hyperlinks](../../../hyperlinks.md#explicit-hyperlinks).

The value is a list of the program and its arguments. In each argument,
`{path}` is replaced by the absolute path of the file, `{line}` by the line
number and `{column}` by the column number. Line and column are `1` when the
link doesn't include them.

```lua
-- Visual Studio Code
config.file_link_editor = { 'code', '--goto', '{path}:{line}:{column}' }
```

On Windows, give the full name of a batch file wrapper, such as `code.cmd`:

```lua
config.file_link_editor = { 'code.cmd', '--goto', '{path}:{line}:{column}' }
```

Other editors:

```lua
-- Sublime Text
config.file_link_editor = { 'subl', '{path}:{line}:{column}' }
-- Zed
config.file_link_editor = { 'zed', '{path}:{line}:{column}' }
-- JetBrains IDEs
config.file_link_editor = { 'idea', '--line', '{line}', '{path}' }
```

When `file_link_editor` is not set, files are opened with the system's
default application for their type, and the line number is not used.
Directories are always opened with the system's default application.

Relative paths are resolved against the working directory of the pane in
which the link was clicked, and nothing happens if no such file exists.

An [open-uri](../window-events/open-uri.md) event handler runs first and can
take over any link, including links to files.
