# WatchXY

<p align="center">
<img src="images/logo.png" width="200" alt="watchxy" title="watchxy" />
</p>

Modern `watch` command.

**WatchXY** is a terminal UI that runs a command on an interval, highlights diffs, and adds a time-machine style history. It started from open-source work by [Takumasa Sakao](https://github.com/sachaos); this tree uses the **watchxy** binary and crate name throughout.

## Demo

<p align="center">
<img src="images/demo.gif" width="100%" alt="watchxy" title="watchxy" />
</p>

## Features

* Basic features of original watch command.
    * Execute command periodically, and display the result.
    * color output.
    * diff highlight.
* Time machine mode. 😎
    * Rewind like video.
    * Go to the past, and back to the future.
* Look back history.
    * Save and load history.
* See output in pager.
* Vim like keymaps.
* Search text.
* Suspend and restart execution.
* Support shell alias (behavior depends on shell; you may need login shells or full profile initialization for aliases defined only in interactive configs).
* Customize keymappings.
* Customize color.

## Install

The installed binary is **`watchxy`**.

### From this repository

```shell
cargo install --path .
```

### crates.io

```shell
cargo install watchxy
```

### [Homebrew](https://brew.sh)

```shell
brew install watchxy
```

### Linux (release tarball)

Adjust `OWNER`, `REPO`, version, and asset name to match your published release:

```shell
wget -O watchxy.tar.gz https://github.com/OWNER/REPO/releases/download/v1.3.0/watchxy-v1.3.0-linux-x86_64.tar.gz \
  && tar xvf watchxy.tar.gz \
  && mv watchxy /usr/local/bin
```

### Other package managers

Community formulas use their own names; search for **watchxy** once a maintainer publishes one.

## Command-line options

```shell
watchxy [OPTIONS] [COMMAND]...
```

Provide at least one of: a positional `COMMAND`, `-C` / `--commands`, or `-F` / `--commands-file`. `--load` restores a saved session and cannot be combined with a command.

| Option | Description |
|--------|-------------|
| `-n`, `--interval` *DURATION* | Wait between updates (default: `2s`). Accepts [humantime](https://docs.rs/humantime) durations; a bare number is treated as seconds. Minimum: `100ms`. |
| `-d`, `--differences` | Highlight changes between updates. |
| `-D`, `--deletion-differences` | Highlight deletions between updates (mutually exclusive with `--differences`). |
| `-p`, `--precise` | Run on precise intervals (compensates for command runtime). |
| `-t`, `--no-title` | Hide the header. |
| `-w`, `--unfold`, `--no-wrap` | Disable line wrapping. |
| `--shell` *SHELL* | Shell for `-c` execution (default: `sh` on Unix, `cmd` on Windows). Conflicts with `--exec`. |
| `--shell-options` *OPTS…* | Extra arguments passed to the shell. |
| `-s`, `--skip-empty-diffs` | Omit history entries when the diff is empty. |
| `-b`, `--bell` | Ring the terminal bell when output changes. |
| `-C`, `--commands` *COMMAND* | Additional watched command (repeatable). Same string form as the positional command. |
| `-F`, `--commands-file` *FILE* | File with one command per line (same as repeating `-C`). Blank lines and `#` comments are ignored. |
| `-x`, `--exec` | Run the command with `exec` instead of `sh -c` (or the configured shell). |
| `--debug` | Enable debug logging. |
| `--save` *FILE* | Write history to this SQLite backup path. |
| `--disable_auto_save` | Do not persist history to disk. |
| `--disable_mouse` | Ignore mouse events. |
| `--load`, `--lookback` *FILE* | Open a backup file (no command or interval flags). |

Examples:

```shell
watchxy -n 1s -d date
watchxy --interval 500ms --precise --bell ./script.sh
watchxy -x -- ./my-binary --flag
watchxy --save ./session.sqlite -n 5 git log -1
watchxy --lookback ./session.sqlite
```

## Multiple commands

Run more than one command and switch between them with `[` and `]`:

```shell
watchxy -n 2s -C "git status" -C "df -h"
watchxy -n 2s git status -C "df -h"
watchxy -n 2s -F commands.txt
```

`commands.txt` is one command per line (same as repeating `-C`). Blank lines and lines starting with `#` are ignored:

```
git status
df -h
# optional comment
```

Each command keeps its own execution history and diff baseline. On startup, every configured command runs once before the interval loop begins. Switching shows the last output for that command when available.

With multiple commands: **SPACE** runs every command now; **r** runs only the active command. With a single command, **SPACE** runs it now.

## Keymaps

| key       |                                            |
|-----------|--------------------------------------------|
| SPACE     | Run command(s) now (all commands when multiple) |
| r         | Run active command now (when multiple)        |
| [ / ]     | Previous / next watched command (when multiple) |
| m         | Toggle time machine mode                   |
| s         | Toggle <ins>s</ins>uspend execution                   |
| b         | Toggle ring terminal <ins>b</ins>ell                  |
| d         | Toggle <ins>d</ins>iff                                |
| t         | Toggle header/<ins>t</ins>itle display                      |
| ?         | Toggle help view                           |
| /         | Search text                                |
| j         | Pager: next line                           |
| k         | Pager: previous line                       |
| h         | Pager: move left                           |
| l         | Pager: move right                          |
| Control-F | Pager: page down                           |
| Control-B | Pager: page up                             |
| g         | Pager: go to top of page                   |
| Shift-G   | Pager: go to bottom of page                |
| Shift-J   | (Time machine mode) Go to the past         |
| Shift-K   | (Time machine mode) Back to the future     |
| Shift-F   | (Time machine mode) Go to more past        |
| Shift-B   | (Time machine mode) Back to more future    |
| Shift-O   | (Time machine mode) Go to oldest position  |
| Shift-N   | (Time machine mode) Go to current position |

## Configuration

WatchXY can be used without any configuration.
However, if you want to customize the keybindings or default behavior, you can do so.

**WatchXY** loads settings in two ways:

1. **Default (recommended):** `config.json5` / `config.toml` / other supported names under the app config directory from XDG / `directories`. Run `watchxy --version` to print **Config directory** and **Data directory** on your machine.
2. **Legacy TOML:** if `$XDG_CONFIG_HOME/watchxy.toml` exists, it is used instead of the JSON-first stack (macOS: `~/Library/Application Support/watchxy.toml`).

```toml
[general]
no_shell = false
shell = "zsh"
shell_options = ""
skip_empty_diffs = false
disable_mouse = true

[keymap]
timemachine_go_to_past = "Down"
timemachine_go_to_more_past = "Shift-Down"
timemachine_go_to_future = "Up"
timemachine_go_to_more_future = "Shift-Up"
timemachine_go_to_now = "Ctrl-Shift-Up"
timemachine_go_to_oldest = "Ctrl-Shift-Down"
scroll_left = "h"
scroll_right = "l"
scroll_up = "k"
scroll_down = "j"
scroll_half_page_up = "Ctrl-u"
scroll_half_page_down = "Ctrl-d"
scroll_page_up = "Ctrl-b"
scroll_page_down = "Ctrl-f"
scroll_bottom_of_page = "Shift-g"
scroll_top_of_page = "g g"

[color]
background = "white" # Default value is inherit from terminal color.
```

## Name and runtime

**WatchXY** is the product name. The binary and crate are **`watchxy`**. Logs and env overrides use that name (for example `watchxy.log`, and `WATCHXY_CONFIG` / `WATCHXY_DATA` / `WATCHXY_LOGLEVEL`).

## Credits

The gopher's logo is licensed under the Creative Commons 3.0 Attributions license.

The original Go gopher was designed by [Renee French](https://reneefrench.blogspot.com/).
