# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

modbus-tui is a Rust TUI application for inspecting, monitoring, and testing Modbus TCP devices. It operates in two modes: **Client** (polls registers from a Modbus device) and **Server** (runs a Modbus TCP server for testing). Built with ratatui + crossterm for the terminal UI and tokio-modbus for the protocol layer.

- **Edition**: Rust 2024, stable toolchain
- **License**: Apache-2.0
- **Repository**: https://github.com/ForestTree/modbus-tui

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

## CI / Release

- **CI** (`.github/workflows/ci.yml`): Runs on push/PR to `main`. Build matrix: `x86_64-unknown-linux-musl` (cross), `x86_64-apple-darwin`, `aarch64-apple-darwin`, `x86_64-pc-windows-msvc`. Plus clippy and rustfmt checks. Uses `dtolnay/rust-toolchain@stable` and `Swatinem/rust-cache@v2`.
- **Release** (`.github/workflows/release.yml`): Triggered by `v*` tags. Builds all 4 targets, strips binaries, packages as `.tar.gz` (Unix) / `.zip` (Windows), creates GitHub Release with auto-generated notes via `softprops/action-gh-release@v3`.
- Version is bumped manually in `Cargo.toml` before tagging.

## Architecture

### Source Layout

```
src/
  main.rs           Entry point, terminal setup, 100ms tick event loop
  config.rs         CLI args (clap derive), JSON config parsing, validation
  app.rs            AppState, SharedState, RegisterValue, LogBuffer, UiState
  format.rs         NumFormat enum, WordSwap, value formatting/parsing
  event.rs          Keyboard handler, command bar (:poll, :export, :save)
  modbus/
    mod.rs          Submodule exports
    client.rs       Async polling loop with exponential backoff reconnect
    server.rs       TCP listener, RegisterStore, Modbus request/response handler
    transport.rs    LoggingTransport wrapper for raw packet capture
  ui/
    mod.rs          Submodule export
    render.rs       All rendering: status bar, register panes, log, dialogs
```

### Data Flow

```
main.rs: Cli::parse() → AppConfig::from_cli() → new_shared_state() → spawn modbus task → run_loop()
```

1. **main.rs** — Parses CLI via clap, creates `AppConfig`, initializes `SharedState`, spawns client or server task, runs the 100ms tick render/input loop with crossterm `EventStream`.
2. **config.rs** — `Cli` struct (clap derive) → `AppConfig::from_cli()`. Supports both CLI args and JSON config files (`-c`). CLI args override config file values when explicitly provided. Parses register ranges as `START:COUNT[:FMT]` with hex (0x prefix) support.
3. **app.rs** — `AppState` holds all runtime state: config, connection status, registers (`Vec<BTreeMap<u16, RegisterValue>>`), log buffer (rolling 500-entry `VecDeque`), UI state (tabs, panes, input modes), server stats. Wrapped as `SharedState = Arc<Mutex<AppState>>`.
4. **modbus/client.rs** — Async polling loop: connects with exponential backoff (1-10s), reads all configured ranges per cycle, drains write requests from UI via MPSC channel, tracks value changes with `changed_at` timestamps.
5. **modbus/server.rs** — TCP listener accepting connections. `RegisterStore` (separate `Arc<std::sync::Mutex>`) handles reads/writes for all register types. Synced to `AppState.registers` each frame via `sync_store_to_registers()`. Tracks `ServerStats` (connections, request counts).
6. **modbus/transport.rs** — `LoggingTransport` wraps `TcpStream`, logs all Tx/Rx bytes as hex strings into `AppState` log buffer. Enabled by `-R`/`--raw-packets`.
7. **event.rs** — Dispatches keyboard events by `InputMode` (6 states: Normal, HelpDialog, FormatDialog, WriteDialog, LabelDialog, CommandBar). Command bar supports `:poll <ms>`, `:export [path]`, `:save [path]`.
8. **format.rs** — `NumFormat` enum (11 variants): `Int16`, `Uint16`, `Int32`, `Uint32`, `Int64`, `Uint64`, `Float16`, `Float32`, `Float64`, `Bin16`, `Ascii`. Each has a `width()` (1/2/4 registers). Custom IEEE 754 half-precision (f16) implementation. `WordSwap` struct handles byte/word-swap with 4 independent flags.
9. **ui/render.rs** — Layout: status bar (3 lines) → register panes (tabbed grid) → log panel (8 lines) → input bar. Register tables show address, hex, value, label columns. Color-coded: white=stable, yellow=recently changed (3s fade), red=stale/error.

### Concurrency Model

Pure async (Tokio). Single `Arc<tokio::sync::Mutex<AppState>>` shared between main loop and modbus task. Communication channels:
- `WriteTx/WriteRx` — unbounded MPSC for UI→client write requests
- `ShutdownTx/Rx` — `watch` channel for graceful shutdown

Short lock scopes throughout — no nested locks. Server mode uses a separate `Arc<std::sync::Mutex<RegisterStore>>` for the register database (blocking mutex since it's accessed from sync Modbus service handlers).

### Key Design Patterns

- **Register grouping**: Table rows grouped by `NumFormat::width()`. f32 (width=2) shows 2 addresses per row, f64 (width=4) shows 4. `selected_addr()` accounts for this grouping.
- **Word-swap**: Multi-register values can have word order reversed via 4 independent flags: `swap_bytes` (within each u16), `swap_ints` (multi-reg integers), `swap_floats` (multi-reg floats), `swap_words` (all multi-reg, overrides ints/floats). Applied symmetrically on read and write.
- **Address translation**: `start_reference` (0 or 1) offsets user-facing addresses from protocol addresses. CLI range parsing subtracts the offset (`user_to_protocol()`). Display adds it back.
- **Change tracking**: `RegisterValue` stores `changed_at`, `stable_since`, `prev_raw`. While value keeps changing, highlight stays active. Once stable, 3-second fade-out begins (`CHANGE_HIGHLIGHT_SECS`).
- **Server sync**: Server mode uses a separate `RegisterStore`; each UI frame copies it into `AppState.registers` via `sync_store_to_registers()` to trigger change detection.
- **Windows key dedup**: Only `KeyEventKind::Press` is handled (Windows emits Press+Release, others only Press).
- **Config save/load roundtrip**: `:save` captures current pane formats, labels, and address display preferences back into `AppConfig` and serializes to JSON.

## Dependencies

| Crate          | Version                         | Purpose                           |
|----------------|---------------------------------|-----------------------------------|
| `ratatui`      | 0.30                            | TUI framework                     |
| `crossterm`    | 0.29.0 (+event-stream)          | Terminal handling, keyboard input |
| `tokio`        | 1 (+full)                       | Async runtime                     |
| `tokio-modbus` | 0.17 (+tcp, server, tcp-server) | Modbus TCP protocol               |
| `clap`         | 4 (+derive)                     | CLI argument parsing              |
| `anyhow`       | 1                               | Error handling                    |
| `serde`        | 1 (+derive)                     | Serialization                     |
| `serde_json`   | 1                               | JSON config/export                |
| `futures-util` | 0.3                             | Async stream utilities            |
| `chrono`       | 0.4                             | Wall-clock timestamps             |

No dev-dependencies. No test suite yet.

## Conventions

- Rust edition 2024, stable toolchain
- No `unsafe` code
- Error handling: `anyhow` for main/config, `Result<(), String>` in modbus/format operations
- User-facing terminology: "unit" (not "slave") for Modbus unit ID
- Register types in config/code: `HoldingRegisters`, `InputRegisters`, `Coils`, `DiscreteInputs`
- CLI short flags: `-H` host, `-P` port, `-u` unit, `-p` poll, `-r` reference, `-i`/`-f`/`-w`/`-b` swap, `-D` decimal, `-n` no-hex, `-R` raw packets, `-c` config, `-m` mode
- CLI register aliases: `--hr` (holding-registers), `--ir` (input-registers), `--co` (coils), `--di` (discrete-inputs)
- Numeric format codes used in CLI and config: `u16`, `i16`, `u32`, `i32`, `u64`, `i64`, `f32`, `f64`, `b16`, `ascii`
- Keyboard shortcuts: `q`/`Esc` quit, `Tab`/`Shift-Tab` switch panes, `j`/`k` or arrows navigate, `f` format dialog, `w` write dialog, `l` label dialog, `d`/`D` decimal addr (pane/all), `h`/`H` hex addr (pane/all), `:` command bar, `F1` help, `F2` toggle register/log focus
- Writable types: `Coils` and `HoldingRegisters` in client mode; all types in server mode
