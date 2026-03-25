# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

modbus-tui is a Rust TUI application for inspecting, monitoring, and testing Modbus TCP devices. It operates in two modes: **Client** (polls registers from a Modbus device) and **Server** (runs a Modbus TCP server for testing). Built with ratatui + crossterm for the terminal UI and tokio-modbus for the protocol layer.

## Build Commands

```sh
cargo build                                    # Dev build
cargo build --release                          # Release build
cargo clippy --all-targets -- -D warnings      # Lint (CI enforces zero warnings)
cargo fmt --check                              # Format check
cargo fmt                                      # Auto-format
cargo test --release                           # Run tests (no test suite yet)
cargo install --path .                         # Install binary locally
```

CI runs on push/PR to main: build (Linux-musl/macOS-x86+arm/Windows), clippy, rustfmt.

## Architecture

### Data Flow

```
main.rs: CLI parse → AppConfig → SharedState(Arc<Mutex<AppState>>) → spawn modbus task → run_loop
```

- **main.rs** — Entry point. Sets up terminal, spawns modbus client or server task, runs the 100ms tick event loop.
- **config.rs** — CLI args (clap derive) and JSON config parsing. `Cli` → `AppConfig::from_cli()` with validation.
- **app.rs** — `AppState` holds all shared state: config, connection status, register data (`Vec<BTreeMap<u16, RegisterValue>>`), log buffer, UI state. Wrapped in `SharedState = Arc<Mutex<AppState>>`.
- **modbus/client.rs** — Async polling loop: connect with exponential backoff, read all configured ranges each cycle, drain write requests from UI via MPSC channel.
- **modbus/server.rs** — TCP listener accepting connections. `RegisterStore` (separate `Arc<Mutex>`) synced to `AppState.registers` each frame via `sync_store_to_registers()`.
- **event.rs** — Keyboard handler. Six `InputMode` states: Normal, HelpDialog, FormatDialog, WriteDialog, LabelDialog, CommandBar.
- **format.rs** — `NumFormat` enum (10 variants: u16/i16/u32/i32/u64/i64/f32/f64/f16/bin16). Handles format/parse with word-swap support. Custom IEEE 754 half-precision (f16) implementation.
- **ui/render.rs** — All rendering. Layout: status bar → register panes (grid) → log panel → input bar. Register panes show tables with rows grouped by `NumFormat::width()` (1/2/4 registers per row).

### Concurrency Model

Pure async (Tokio). Single `Arc<Mutex<AppState>>` shared between main loop and modbus task. Communication: `WriteTx/WriteRx` unbounded MPSC for UI→client writes, `ShutdownTx/Rx` watch channel for graceful stop. Short lock scopes throughout — no nested locks.

### Key Design Patterns

- **Register grouping**: Rows in the table are grouped by `NumFormat::width()`. A f32 format (width=2) shows 2 addresses per row. `selected_addr()` accounts for this grouping.
- **Word-swap**: Multi-register values can have word order reversed independently for ints vs floats (`swap_ints`/`swap_floats`). Applied symmetrically on read and write.
- **Address translation**: `start_reference` (0 or 1) offsets user-facing addresses from protocol addresses. Applied at config parse time for protocol ops, at display time for UI.
- **Change tracking**: `RegisterValue` stores `changed_at: Option<Instant>` and highlights for 3 seconds with color fade-out.
- **Server sync**: Server mode uses a separate `RegisterStore`; each UI frame copies it into `AppState.registers` to trigger change detection.
- **Windows key dedup**: Only `KeyEventKind::Press` is handled (Windows emits Press+Release, others only Press).

## Conventions

- Rust edition 2024, stable toolchain
- No `unsafe` code
- Error handling via `anyhow` (main/config), `Result<(), String>` in modbus operations
- User-facing terminology: "unit" (not "slave") for Modbus unit ID
- Register types in config/code: `HoldingRegisters`, `InputRegisters`, `Coils`, `DiscreteInputs`
- CLI short flags follow a pattern: `-H` host, `-P` port, `-u` unit, `-p` poll, `-r` reference, `-i`/`-f` swap, `-D` decimal, `-n` no-hex, `-c` config file, `-m` mode
