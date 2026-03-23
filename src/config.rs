use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::{Parser, ValueEnum};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// CLI definition
// ---------------------------------------------------------------------------

#[derive(Debug, Parser)]
#[command(name = "modbus-tui", about = "Modbus TCP TUI client/server")]
pub struct Cli {
    /// Run as client or server
    #[arg(short, long, default_value = "client")]
    pub mode: Mode,

    /// Target host or IP address
    #[arg(short = 'H', long, default_value = "127.0.0.1")]
    pub host: String,

    /// TCP port
    #[arg(short = 'P', long, default_value_t = 502)]
    pub port: u16,

    /// Modbus slave / unit ID (0-247)
    #[arg(short = 'u', long, default_value_t = 1)]
    pub unit: u8,

    /// Coil register range: START:COUNT (e.g. 0:10) [alias: --co] (repeatable)
    #[arg(long, alias = "co", value_name = "START:COUNT", action = clap::ArgAction::Append)]
    pub coils: Vec<String>,

    /// Discrete input range: START:COUNT [alias: --di] (repeatable)
    #[arg(long, alias = "di", value_name = "START:COUNT", action = clap::ArgAction::Append)]
    pub discrete_inputs: Vec<String>,

    /// Holding register range: START:COUNT[:FMT] where FMT = u16|i16|u32|i32|u64|i64|f32|f64|b16 [alias: --hr] (repeatable)
    #[arg(long, alias = "hr", value_name = "START:COUNT[:FMT]", action = clap::ArgAction::Append)]
    pub holding_registers: Vec<String>,

    /// Input register range: START:COUNT[:FMT] where FMT = u16|i16|u32|i32|u64|i64|f32|f64|b16 [alias: --ir] (repeatable)
    #[arg(long, alias = "ir", value_name = "START:COUNT[:FMT]", action = clap::ArgAction::Append)]
    pub input_registers: Vec<String>,

    /// Start reference: 0 = zero-based (0 to user, 0 to protocol), 1 = one-based addressing (1 to user, 0 to protocol)
    #[arg(short = 'r', long, default_value_t = 0, value_parser = clap::value_parser!(u16).range(0..=1))]
    pub start_reference: u16,

    /// Word-swap 32/64-bit integers (big-endian word order → swapped)
    #[arg(short = 'i', long)]
    pub swap_ints: bool,

    /// Word-swap 32/64-bit floats (big-endian word order → swapped)
    #[arg(short = 'f', long)]
    pub swap_floats: bool,

    /// Poll interval in milliseconds (10..60000)
    #[arg(short = 'p', long, default_value_t = 100)]
    pub poll_interval: u64,

    /// Show MODBUS addresses in decimal (instead of hex) for all panes
    #[arg(short = 'D', long)]
    pub decimal_addresses: bool,

    /// Path to a JSON config file (overrides other flags)
    #[arg(short = 'c', long, value_name = "FILE")]
    pub config: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Client,
    Server,
}

// ---------------------------------------------------------------------------
// Register type + range
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RegisterType {
    Coils,
    DiscreteInputs,
    HoldingRegisters,
    InputRegisters,
}

impl RegisterType {
    pub fn is_coil_type(self) -> bool {
        matches!(self, Self::Coils | Self::DiscreteInputs)
    }

    pub fn is_writable(self) -> bool {
        matches!(self, Self::Coils | Self::HoldingRegisters)
    }
}

impl fmt::Display for RegisterType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Coils => write!(f, "Coils"),
            Self::DiscreteInputs => write!(f, "Discrete Inputs"),
            Self::HoldingRegisters => write!(f, "Holding Registers"),
            Self::InputRegisters => write!(f, "Input Registers"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PollRange {
    pub reg_type: RegisterType,
    pub start: u16,
    pub count: u16,
    /// Optional initial numeric format for the pane (word registers only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial_format: Option<crate::format::NumFormat>,
}

impl PollRange {
    /// Tab display label using user-facing address (with start_reference offset)
    /// and the given address format.
    pub fn tab_label(&self, start_reference: u16, addr_format: crate::app::AddrFormat) -> String {
        let display_start = self.start + start_reference;
        let addr_str = match addr_format {
            crate::app::AddrFormat::Hex => format!("0x{:04X}", display_start),
            crate::app::AddrFormat::Decimal => format!("{}", display_start),
        };
        format!("{} @{}", self.reg_type, addr_str)
    }
}

// ---------------------------------------------------------------------------
// Validated application config
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub mode: Mode,
    pub host: String,
    pub port: u16,
    pub unit: u8,
    /// Ordered list of register ranges to poll / display as tabs.
    /// `start` is always the protocol address (0-based).
    pub ranges: Vec<PollRange>,
    pub poll_interval_ms: u64,
    /// 0 = zero-based addressing, 1 = one-based MODBUS addressing.
    /// User-facing addresses = protocol address + start_reference.
    #[serde(default)]
    pub start_reference: u16,
    /// Word-swap multi-register integers during conversion.
    #[serde(default)]
    pub swap_ints: bool,
    /// Word-swap multi-register floats during conversion.
    #[serde(default)]
    pub swap_floats: bool,
    /// Server mode: initial register values keyed by "type:address" (e.g. "hr:0": 1234).
    #[serde(default)]
    pub initial_values: HashMap<String, u16>,
}

// ---------------------------------------------------------------------------
// Parsing & validation
// ---------------------------------------------------------------------------

impl AppConfig {
    pub fn from_cli(cli: &Cli) -> Result<Self> {
        let mut cfg = if let Some(ref path) = cli.config {
            Self::load(path)?
        } else {
            let sr = cli.start_reference;

            // Build ranges in the same order as they appear on the command line
            // by scanning raw args for the register-type flags.
            let mut ranges = Vec::new();
            let mut co_idx = 0usize;
            let mut di_idx = 0usize;
            let mut hr_idx = 0usize;
            let mut ir_idx = 0usize;

            let raw_args: Vec<String> = std::env::args().collect();
            for arg in &raw_args {
                match arg.as_str() {
                    "--coils" | "--co" => {
                        if let Some(s) = cli.coils.get(co_idx) {
                            let (user_start, count) = parse_range(s, "coils")?;
                            let start = user_to_protocol(user_start, sr, "coils")?;
                            ranges.push(PollRange {
                                reg_type: RegisterType::Coils,
                                start,
                                count,
                                initial_format: None,
                            });
                            co_idx += 1;
                        }
                    }
                    "--discrete-inputs" | "--di" => {
                        if let Some(s) = cli.discrete_inputs.get(di_idx) {
                            let (user_start, count) = parse_range(s, "discrete-inputs")?;
                            let start = user_to_protocol(user_start, sr, "discrete-inputs")?;
                            ranges.push(PollRange {
                                reg_type: RegisterType::DiscreteInputs,
                                start,
                                count,
                                initial_format: None,
                            });
                            di_idx += 1;
                        }
                    }
                    "--holding-registers" | "--hr" => {
                        if let Some(s) = cli.holding_registers.get(hr_idx) {
                            let (user_start, count, fmt) =
                                parse_range_with_format(s, "holding-registers")?;
                            let start = user_to_protocol(user_start, sr, "holding-registers")?;
                            ranges.push(PollRange {
                                reg_type: RegisterType::HoldingRegisters,
                                start,
                                count,
                                initial_format: fmt,
                            });
                            hr_idx += 1;
                        }
                    }
                    "--input-registers" | "--ir" => {
                        if let Some(s) = cli.input_registers.get(ir_idx) {
                            let (user_start, count, fmt) =
                                parse_range_with_format(s, "input-registers")?;
                            let start = user_to_protocol(user_start, sr, "input-registers")?;
                            ranges.push(PollRange {
                                reg_type: RegisterType::InputRegisters,
                                start,
                                count,
                                initial_format: fmt,
                            });
                            ir_idx += 1;
                        }
                    }
                    _ => {}
                }
            }

            Self {
                mode: cli.mode,
                host: cli.host.clone(),
                port: cli.port,
                unit: cli.unit,
                ranges,
                poll_interval_ms: cli.poll_interval,
                start_reference: sr,
                swap_ints: cli.swap_ints,
                swap_floats: cli.swap_floats,
                initial_values: HashMap::new(),
            }
        };

        cfg.validate()?;
        Ok(cfg)
    }

    fn load(path: &Path) -> Result<Self> {
        let contents =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let config: Self = serde_json::from_str(&contents)
            .with_context(|| format!("parsing {}", path.display()))?;
        Ok(config)
    }

    fn validate(&mut self) -> Result<()> {
        if self.port == 0 {
            bail!("port must be in range 1..65535");
        }
        if self.unit > 247 {
            bail!("unit id must be in range 0..247, got {}", self.unit);
        }
        if self.poll_interval_ms < 10 || self.poll_interval_ms > 60_000 {
            bail!(
                "poll interval must be between 10 and 60000 ms, got {}",
                self.poll_interval_ms
            );
        }
        for (i, r) in self.ranges.iter().enumerate() {
            let label = format!(
                "range[{}] ({})",
                i,
                r.tab_label(self.start_reference, crate::app::AddrFormat::default())
            );
            if r.count == 0 {
                bail!("{label}: count must be > 0");
            }
            if (r.start as u32) + (r.count as u32) > 65536 {
                bail!(
                    "{label}: start ({}) + count ({}) exceeds address space (max 65536)",
                    r.start,
                    r.count
                );
            }
        }
        Ok(())
    }
}

/// Parse "START:COUNT" into (start, count). Start is the user-facing address.
fn parse_range(s: &str, name: &str) -> Result<(u16, u16)> {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() != 2 {
        bail!("{name}: expected format START:COUNT, got \"{s}\"");
    }
    let start: u16 = parts[0]
        .parse()
        .with_context(|| format!("{name}: invalid start address \"{}\"", parts[0]))?;
    let count: u16 = parts[1]
        .parse()
        .with_context(|| format!("{name}: invalid count \"{}\"", parts[1]))?;
    Ok((start, count))
}

/// Parse "START:COUNT" or "START:COUNT:FMT" where FMT is a NumFormat code.
fn parse_range_with_format(
    s: &str,
    name: &str,
) -> Result<(u16, u16, Option<crate::format::NumFormat>)> {
    let parts: Vec<&str> = s.split(':').collect();
    match parts.len() {
        2 => {
            let (start, count) = parse_range(s, name)?;
            Ok((start, count, None))
        }
        3 => {
            let range_str = format!("{}:{}", parts[0], parts[1]);
            let (start, count) = parse_range(&range_str, name)?;
            let nf: crate::format::NumFormat = parts[2]
                .parse()
                .map_err(|e: String| anyhow::anyhow!("{name}: {e}"))?;
            Ok((start, count, Some(nf)))
        }
        _ => bail!("{name}: expected format START:COUNT[:FMT], got \"{s}\""),
    }
}

/// Convert user-facing address to protocol address by subtracting start_reference.
fn user_to_protocol(user_addr: u16, start_reference: u16, name: &str) -> Result<u16> {
    if user_addr < start_reference {
        bail!("{name}: address {user_addr} is below start reference {start_reference}");
    }
    Ok(user_addr - start_reference)
}
