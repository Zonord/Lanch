# Agent Instructions for lanch

## Project Overview
A minimal TUI (Terminal User Interface) application launcher for Linux. It reads `.desktop` files from standard XDG paths, provides a searchable list, and launches the selected app.

## Tech Stack
- **Language**: Rust (Edition 2024)
- **TUI Framework**: `ratatui` and `crossterm`
- **Desktop Protocol**: `freedesktop-desktop-entry`
- **System Integration**: `libc` (used for process session detachment)

## Developer Commands
- **Build Release**: `cargo build --release`
- **Install**: `install -m 755 target/release/lanch ~/.local/bin/lanch`

## Key Implementation Details & Quirks
- **Process Detachment**: When launching an application, `libc::setsid()` is called in `pre_exec` to ensure the child process is a session leader and survives the TUI's termination.
- **Field Code Stripping**: The `launch` function automatically strips XDG field codes (e.g., `%u`, `%f`, `%i`) from the `Exec` line of `.desktop` files.
- **Search Algorithm**: Match priority is strictly ordered: `Exact` > `Prefix` > `Substring` > `Subsequence` > `Fuzzy`.
- **Error Reporting**: If an application fails to spawn, an error message is written to `/tmp/launch.err`.

## Architecture
- **Monolith**: The entire application logic is contained in `src/main.rs`.
- **Execution Flow**: `main` calls `ratatui::run` -> `run` (loop) -> `handle_key` / `draw_ui`.
