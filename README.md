# lanch

A minimal TUI application launcher for Linux. It reads `.desktop` files from standard XDG paths, shows a searchable list, and launches the selected app on Enter.

Built with [ratatui](https://github.com/ratatui/ratatui) and [freedesktop-desktop-entry](https://crates.io/crates/freedesktop-desktop-entry).

## Features

- Lists all visible applications from standard XDG directories
- Localized names via `get_languages_from_env`
- Live substring search (case-insensitive)
- Arrow key navigation, wrapping around at the ends
- Launches via `exec()` — no forks, no extra processes
- Strips field codes (`%u`, `%U`, `%f`, `%F`, `%i`, `%c`, …) from the `Exec` field

## Installation

```sh
cargo build --release
install -m 755 /target/release/lanch ~/.local/bin/lanch
