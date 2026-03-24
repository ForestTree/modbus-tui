# modbus-tui

A terminal-based Modbus TCP client and server for inspecting, monitoring, and testing Modbus devices.

## Features

- **Client mode** — connect to a Modbus TCP device and poll registers in real time
- **Server mode** — run a Modbus TCP server with configurable registers for testing
- **Multiple register types** — Coils, Discrete Inputs, Holding Registers, Input Registers
- **Numeric formats** — view registers as u16, i16, u32, i32, u64, i64, f32, f64, or binary
- **Multi-pane layout** — display multiple register ranges side by side
- **Write support** — modify register values from the TUI (all types in server mode)
- **Change highlighting** — recently changed values are color-highlighted with fade-out
- **Register labels** — assign custom labels to individual registers
- **Word-swap** — configurable byte order for multi-register integers and floats
- **JSON config** — load register layout from a JSON configuration file
- **Export** — export current register values to JSON
- **Cross-platform** — runs on Linux, macOS, and Windows

## Installation

```sh
cargo install --path .
```

Or build from source:

```sh
cargo build --release
```

## Usage

### Client mode

Connect to a Modbus TCP device and monitor registers:

```sh
# Read 10 holding registers starting at address 0
modbus-tui --hr 0:10

# Multiple register ranges with custom format
modbus-tui -H 192.168.1.100 -P 502 --hr 0:10:f32 --hr 100:20 --ir 0:8 --co 0:16

# One-based addressing, decimal display
modbus-tui -r 1 -D --hr 1:10

# Custom poll interval (500ms)
modbus-tui -p 500 --hr 0:10
```

### Server mode

Run a Modbus TCP server for testing:

```sh
# Server with holding registers and coils
modbus-tui -m server --hr 0:10 --co 0:16

# Server on a custom port with all register types
modbus-tui -m server -P 5020 --hr 0:20 --ir 0:10 --co 0:8 --di 0:8
```

### JSON configuration

```sh
modbus-tui -c config.json
```

Example `config.json`:

```json
{
  "mode": "client",
  "host": "192.168.1.100",
  "port": 502,
  "unit": 1,
  "poll_interval_ms": 200,
  "start_reference": 0,
  "ranges": [
    { "reg_type": "HoldingRegisters", "start": 0, "count": 10, "initial_format": "Float32" },
    { "reg_type": "InputRegisters", "start": 0, "count": 8 },
    { "reg_type": "Coils", "start": 0, "count": 16 }
  ],
  "initial_values": {
    "hr:0": 1234,
    "co:0": 1
  }
}
```

## Keyboard shortcuts

| Key | Action |
|-----|--------|
| `q` / `Esc` | Quit |
| `Tab` / `Shift+Tab` | Switch between register panes |
| `F2` | Toggle focus between registers and log |
| `j` / `k` / `Up` / `Down` | Navigate rows |
| `PageUp` / `PageDown` | Scroll by 20 rows |
| `Home` / `End` | Jump to top / bottom |
| `w` | Write value to selected register |
| `f` | Change numeric format for current pane |
| `l` | Edit label for selected register |
| `d` / `D` | Decimal addresses (current pane / all) |
| `h` / `H` | Hex addresses (current pane / all) |
| `:` | Open command bar |
| `F1` | Help |

### Commands

| Command | Description |
|---------|-------------|
| `:poll <ms>` | Change poll interval (10–60000 ms) |
| `:export [path]` | Export registers to JSON (default: `registers.json`) |

## License

Apache License 2.0 — see [LICENSE](LICENSE) for details.
