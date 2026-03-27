# modbus-tui

A terminal-based Modbus TCP client and server for inspecting, monitoring, and testing Modbus devices.

![modbus-tui](assets/screenshot.png)

## Features

- **Client mode** — connect to a Modbus TCP device and poll registers in real time
- **Server mode** — run a Modbus TCP server with configurable registers for testing
- **Multiple register types** — Coils, Discrete Inputs, Holding Registers, Input Registers
- **Numeric formats** — view registers as i16, u16, i32, u32, i64, u64, f16, f32, f64, or binary
- **Multi-pane layout** — display multiple register ranges side by side
- **Write support** — modify holding registers and coils in client mode; all types in server mode
- **Change highlighting** — actively changing values stay highlighted; fades out after value stabilizes
- **Register labels** — assign custom labels to individual registers
- **Byte/word-swap** — configurable byte and word order for all register types
- **JSON config** — load and save full configuration including formats and labels
- **Export** — export current register values to JSON
- **Cross-platform** — runs on Linux, macOS, and Windows

## Platform Support

modbus-tui runs natively on all major platforms:

| Platform | Architecture |
|----------|-------------|
| Linux | x86_64 (musl) |
| macOS | x86_64, ARM (Apple Silicon) |
| Windows | x86_64 |

Pre-built binaries for all platforms are available on the [Releases](https://github.com/ForestTree/modbus-tui/releases) page.

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

# Custom poll interval (500ms), hide hex column
modbus-tui -p 500 -n --hr 0:10

# Word-swap floats and integers
modbus-tui -i -f --hr 0:10:f32

# Byte-swap all registers (reverse bytes within each u16)
modbus-tui -b --hr 0:10

# Combine byte-swap with word-swap
modbus-tui -b -i -f --hr 0:10:f32
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
# Load configuration from file
modbus-tui -c config.json

# Override host and port from config via CLI
modbus-tui -c config.json -H 10.0.0.1 -P 1502
```

Only `ranges` is required in the config file — all other fields have defaults and can be overridden with CLI arguments.

Example `config.json`:

```json
{
  "mode": "client",
  "host": "192.168.1.100",
  "port": 502,
  "unit": 1,
  "poll_interval_ms": 200,
  "start_reference": 0,
  "swap_ints": false,
  "swap_floats": false,
  "swap_bytes": false,
  "hide_hex": false,
  "decimal_addresses": false,
  "ranges": [
    {
      "reg_type": "holdingregisters",
      "start": 0,
      "count": 10,
      "initial_format": "Float32",
      "labels": { "0": "Temperature", "2": "Pressure" }
    },
    { "reg_type": "inputregisters", "start": 0, "count": 8 },
    { "reg_type": "coils", "start": 0, "count": 16 }
  ],
  "initial_values": {
    "hr:0": 1234,
    "co:0": 1
  }
}
```

Config fields and defaults:

| Field | Default | Description |
|-------|---------|-------------|
| `mode` | `"client"` | `"client"` or `"server"` |
| `host` | `"127.0.0.1"` | Target host or IP address |
| `port` | `502` | TCP port |
| `unit` | `1` | Modbus unit ID (0–247) |
| `poll_interval_ms` | `100` | Poll interval in milliseconds (10–60000) |
| `start_reference` | `0` | `0` = zero-based, `1` = one-based addressing |
| `swap_ints` | `false` | Word-swap 32/64-bit integers |
| `swap_floats` | `false` | Word-swap 32/64-bit floats |
| `swap_bytes` | `false` | Byte-swap all registers (reverse bytes within each u16) |
| `hide_hex` | `false` | Hide raw hex column |
| `decimal_addresses` | `false` | Show addresses in decimal |
| `ranges` | `[]` | Register ranges to poll/display |
| `initial_values` | `{}` | Server mode: initial values (`"hr:0": 1234`, `"co:0": 1`) |

Range object fields:

| Field | Required | Description |
|-------|----------|-------------|
| `reg_type` | yes | `"holdingregisters"`, `"inputregisters"`, `"coils"`, or `"discreteinputs"` |
| `start` | yes | Start address (protocol, 0-based) |
| `count` | yes | Number of registers |
| `initial_format` | no | Numeric format: `"Int16"`, `"Uint16"`, `"Int32"`, `"Uint32"`, `"Int64"`, `"Uint64"`, `"Float16"`, `"Float32"`, `"Float64"`, `"Bin16"` |
| `labels` | no | Map of protocol address to label string |

## CLI arguments

### Connection

| Flag | Long | Default | Description |
|------|------|---------|-------------|
| `-m` | `--mode` | `client` | Run as `client` or `server` |
| `-H` | `--host` | `127.0.0.1` | Target host or IP address |
| `-P` | `--port` | `502` | TCP port |
| `-u` | `--unit` | `1` | Modbus unit ID (0–247) |

### Register ranges (repeatable)

| Flag | Long / Alias | Format | Description |
|------|--------------|--------|-------------|
| | `--holding-registers` / `--hr` | `START:COUNT[:FMT]` | Holding register range with optional format |
| | `--input-registers` / `--ir` | `START:COUNT[:FMT]` | Input register range with optional format |
| | `--coils` / `--co` | `START:COUNT` | Coil range |
| | `--discrete-inputs` / `--di` | `START:COUNT` | Discrete input range |

Format codes (`FMT`): `u16`, `i16`, `u32`, `i32`, `u64`, `i64`, `f32`, `f64`, `b16`

### Display options

| Flag | Long | Description |
|------|------|-------------|
| `-r` | `--start-reference` | Address reference: `0` = zero-based, `1` = one-based (default: `0`) |
| `-D` | `--decimal-addresses` | Show addresses in decimal instead of hex |
| `-n` | `--no-hex` | Hide raw hex column |

### Byte/word-swap

| Flag | Long | Description |
|------|------|-------------|
| `-b` | `--swap-bytes` | Byte-swap all registers (reverse bytes within each u16: `0xABCD` → `0xCDAB`) |
| `-i` | `--swap-ints` | Word-swap 32/64-bit integers (reverse register order) |
| `-f` | `--swap-floats` | Word-swap 32/64-bit floats (reverse register order) |

### Other

| Flag | Long | Default | Description |
|------|------|---------|-------------|
| `-p` | `--poll-interval` | `100` | Poll interval in ms (10–60000) |
| `-c` | `--config` | | Path to JSON config file |

When `-c` is used, CLI arguments (`-H`, `-P`, `-u`, `-m`, `-p`) override values from the config file.

## Keyboard shortcuts

| Key | Action |
|-----|--------|
| `q` / `Esc` | Quit |
| `Ctrl+C` | Quit |
| `Tab` / `Shift+Tab` | Switch between register panes |
| `F2` | Toggle focus between registers and log |
| `j` / `k` / `Up` / `Down` | Navigate rows |
| `PageUp` / `PageDown` | Scroll by 20 rows |
| `Home` / `End` | Jump to first / last row |
| `w` | Write value to selected register |
| `f` | Choose numeric format for active pane |
| `l` | Edit label for selected register |
| `d` | Switch to decimal addresses (active pane) |
| `D` | Switch to decimal addresses (all panes) |
| `h` | Switch to hex addresses (active pane) |
| `H` | Switch to hex addresses (all panes) |
| `:` | Open command bar |
| `F1` | Help |

### Commands

| Command | Description |
|---------|-------------|
| `:poll <ms>` | Change poll interval (10–60000 ms) |
| `:export [path]` | Export registers to JSON (default: `registers.json`) |
| `:save [path]` | Save current config for `-c` option (default: `config.json`) |

## License

Apache License 2.0 — see [LICENSE](LICENSE) for details.
