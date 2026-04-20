use std::collections::{BTreeMap, VecDeque};
use std::fmt;
use std::sync::Arc;
use std::time::Instant;

use chrono::Local;
use tokio::sync::Mutex;

use crate::config::{AppConfig, Mode};
use crate::format::NumFormat;

// ---------------------------------------------------------------------------
// Shared handle
// ---------------------------------------------------------------------------

pub type SharedState = Arc<Mutex<AppState>>;

pub type ShutdownTx = tokio::sync::watch::Sender<bool>;
pub type ShutdownRx = tokio::sync::watch::Receiver<bool>;

/// Write request sent from the UI to the client task.
#[derive(Debug, Clone)]
pub struct WriteRequest {
    /// Index into `config.ranges` / `registers` vec.
    pub tab_index: usize,
    pub addr: u16,
    /// One or more u16 register values (big-endian word order for multi-register writes).
    pub values: Vec<u16>,
}

pub type WriteTx = tokio::sync::mpsc::UnboundedSender<WriteRequest>;
pub type WriteRx = tokio::sync::mpsc::UnboundedReceiver<WriteRequest>;

pub fn new_shared_state(config: AppConfig) -> (SharedState, ShutdownTx, WriteTx, WriteRx) {
    let state = Arc::new(Mutex::new(AppState::new(config)));
    let (shutdown_tx, _) = tokio::sync::watch::channel(false);
    let (write_tx, write_rx) = tokio::sync::mpsc::unbounded_channel();
    (state, shutdown_tx, write_tx, write_rx)
}

// ---------------------------------------------------------------------------
// Top-level application state
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct AppState {
    pub running: bool,
    pub config: AppConfig,
    pub connection: ConnectionStatus,
    /// One BTreeMap per entry in `config.ranges`, indexed in parallel.
    pub registers: Vec<BTreeMap<u16, RegisterValue>>,
    pub log: LogBuffer,
    pub ui: UiState,
    pub server: ServerStats,
    pub write_tx: Option<WriteTx>,
    /// Server mode: shared register store for direct reads/writes.
    pub server_store: Option<Arc<std::sync::Mutex<crate::modbus::server::RegisterStore>>>,
    /// Client-only: rotating spinner index, incremented on each successful poll cycle.
    pub spinner_tick: u8,
}

#[derive(Debug, Default)]
pub struct ServerStats {
    pub active_connections: usize,
    pub total_connections: u64,
    pub requests_coils: u64,
    pub requests_discrete_inputs: u64,
    pub requests_holding_registers: u64,
    pub requests_input_registers: u64,
    pub requests_write: u64,
    pub requests_other: u64,
}

impl AppState {
    pub fn new(config: AppConfig) -> Self {
        let n = config.ranges.len();
        Self {
            running: true,
            registers: vec![BTreeMap::new(); n],
            connection: ConnectionStatus::Disconnected,
            log: LogBuffer::new(),
            ui: UiState::new(n),
            server: ServerStats::default(),
            write_tx: None,
            server_store: None,
            spinner_tick: 0,
            config,
        }
    }

    /// Number of tabs (= number of configured ranges).
    pub fn tab_count(&self) -> usize {
        self.config.ranges.len()
    }

    /// Apply per-range config defaults: initial numeric format for each pane,
    /// `decimal_addresses`, and pre-populate register maps with every address
    /// in the range so label placeholders don't cause render-time chunk-size
    /// mismatches.
    pub fn apply_range_defaults(&mut self) {
        if self.config.decimal_addresses {
            for p in &mut self.ui.panes {
                p.addr_format = AddrFormat::Decimal;
            }
        }
        let formats: Vec<_> = self
            .config
            .ranges
            .iter()
            .map(|r| r.initial_format)
            .collect();
        for (i, fmt) in formats.into_iter().enumerate() {
            if let Some(nf) = fmt
                && let Some(p) = self.ui.panes.get_mut(i)
            {
                p.num_format = nf;
            }
        }
        let ranges: Vec<_> = self
            .config
            .ranges
            .iter()
            .map(|r| (r.start, r.count, r.labels.clone()))
            .collect();
        for (i, (start, count, labels)) in ranges.into_iter().enumerate() {
            for offset in 0..count {
                let addr = start + offset;
                self.registers[i]
                    .entry(addr)
                    .or_insert_with(|| RegisterValue::new(0));
            }
            for (addr, label) in labels {
                if let Some(rv) = self.registers[i].get_mut(&addr) {
                    rv.label = Some(label);
                }
            }
        }
    }

    /// Capture the current UI state (pane formats, labels, address display)
    /// back into an `AppConfig` suitable for JSON serialization. Pure: does
    /// not touch the filesystem.
    pub fn build_saved_config(&self) -> AppConfig {
        let mut config = self.config.clone();

        for (i, pane) in self.ui.panes.iter().enumerate() {
            if let Some(range) = config.ranges.get_mut(i) {
                if !range.reg_type.is_coil_type() {
                    range.initial_format = Some(pane.num_format);
                }
                let mut labels = BTreeMap::new();
                if let Some(regs) = self.registers.get(i) {
                    for (&addr, rv) in regs {
                        if let Some(label) = &rv.label {
                            labels.insert(addr, label.clone());
                        }
                    }
                }
                range.labels = labels;
            }
        }

        config.decimal_addresses = self
            .ui
            .panes
            .iter()
            .all(|p| p.addr_format == AddrFormat::Decimal);

        config
    }

    /// Register map for the currently active tab.
    pub fn registers_for_tab(&self) -> &BTreeMap<u16, RegisterValue> {
        self.registers
            .get(self.ui.active_tab)
            .expect("active_tab out of bounds")
    }

    /// Whether the currently active tab shows a coil-type register.
    pub fn active_tab_is_coils(&self) -> bool {
        self.config
            .ranges
            .get(self.ui.active_tab)
            .map(|r| r.reg_type.is_coil_type())
            .unwrap_or(false)
    }

    /// Whether the currently active tab is writable.
    /// In server mode all register types are writable (testing purposes).
    pub fn active_tab_is_writable(&self) -> bool {
        self.config
            .ranges
            .get(self.ui.active_tab)
            .map(|r| self.config.mode == Mode::Server || r.reg_type.is_writable())
            .unwrap_or(false)
    }

    /// Get the protocol address of the currently selected row in the active pane,
    /// accounting for multi-register format grouping.
    pub fn selected_addr(&self) -> Option<u16> {
        let regs = self.registers_for_tab();
        let pane = self.ui.panes.get(self.ui.active_tab)?;
        let width = if self.active_tab_is_coils() {
            1
        } else {
            pane.num_format.width()
        };
        let addrs: Vec<u16> = regs.keys().copied().collect();
        // Rows are grouped by `width`: row 0 = addrs[0], row 1 = addrs[width], ...
        let grouped_count = addrs.len() / width.max(1);
        let row = pane.selected_row.min(grouped_count.saturating_sub(1));
        addrs.get(row * width).copied()
    }
}

// ---------------------------------------------------------------------------
// Connection status
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionStatus {
    Disconnected,
    Connecting,
    Connected,
    Error(String),
}

impl fmt::Display for ConnectionStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Disconnected => write!(f, "Disconnected"),
            Self::Connecting => write!(f, "Connecting…"),
            Self::Connected => write!(f, "Connected"),
            Self::Error(e) => write!(f, "Error: {e}"),
        }
    }
}

// ---------------------------------------------------------------------------
// Register value — with change tracking
// ---------------------------------------------------------------------------

pub const CHANGE_HIGHLIGHT_SECS: u64 = 3;

#[derive(Debug, Clone)]
pub struct RegisterValue {
    pub raw: u16,
    pub last_read: Instant,
    pub label: Option<String>,
    pub changed_at: Option<Instant>,
    pub prev_raw: Option<u16>,
    pub changed_wall: String,
    /// Set when the value stops changing (first poll that reads the same value).
    /// While `None`, the value is actively changing and stays highlighted.
    pub stable_since: Option<Instant>,
}

impl RegisterValue {
    pub fn new(raw: u16) -> Self {
        Self {
            raw,
            last_read: Instant::now(),
            label: None,
            changed_at: None,
            prev_raw: None,
            changed_wall: Local::now().format("%Y-%m-%d %H:%M:%S%.3f").to_string(),
            stable_since: None,
        }
    }

    pub fn update(&mut self, raw: u16) {
        if raw != self.raw {
            self.prev_raw = Some(self.raw);
            self.changed_at = Some(Instant::now());
            self.changed_wall = Local::now().format("%Y-%m-%d %H:%M:%S%.3f").to_string();
            self.stable_since = None;
        } else if self.changed_at.is_some() && self.stable_since.is_none() {
            // Value was changing but now reads the same — start fade-out timer
            self.stable_since = Some(Instant::now());
        }
        self.raw = raw;
        self.last_read = Instant::now();
    }

    pub fn recently_changed(&self) -> bool {
        match self.changed_at {
            None => false,
            Some(_) => match self.stable_since {
                // Still actively changing — always highlighted
                None => true,
                // Stable — fade out after CHANGE_HIGHLIGHT_SECS
                Some(t) => t.elapsed().as_secs() < CHANGE_HIGHLIGHT_SECS,
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Event log
// ---------------------------------------------------------------------------

const LOG_CAPACITY: usize = 500;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogLevel {
    Info,
    Warn,
    Error,
    PacketTx,
    PacketRx,
}

impl fmt::Display for LogLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Info => write!(f, "INFO"),
            Self::Warn => write!(f, "WARN"),
            Self::Error => write!(f, "ERR "),
            Self::PacketTx => write!(f, "TX>>"),
            Self::PacketRx => write!(f, "RX<<"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct LogEntry {
    pub level: LogLevel,
    pub message: String,
    pub wall_clock: String,
}

#[derive(Debug)]
pub struct LogBuffer {
    pub entries: VecDeque<LogEntry>,
}

impl LogBuffer {
    pub fn new() -> Self {
        Self {
            entries: VecDeque::with_capacity(LOG_CAPACITY),
        }
    }

    pub fn push(&mut self, level: LogLevel, message: impl Into<String>) {
        if self.entries.len() == LOG_CAPACITY {
            self.entries.pop_front();
        }
        self.entries.push_back(LogEntry {
            level,
            message: message.into(),
            wall_clock: Local::now().format("%H:%M:%S%.3f").to_string(),
        });
    }

    pub fn info(&mut self, message: impl Into<String>) {
        self.push(LogLevel::Info, message);
    }
    pub fn warn(&mut self, message: impl Into<String>) {
        self.push(LogLevel::Warn, message);
    }
    pub fn error(&mut self, message: impl Into<String>) {
        self.push(LogLevel::Error, message);
    }
}

// ---------------------------------------------------------------------------
// UI state
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusPane {
    Registers,
    Log,
}

#[derive(Debug, Clone)]
pub enum InputMode {
    Normal,
    HelpDialog,
    FormatDialog {
        /// Currently highlighted index in NumFormat::ALL
        selected: usize,
    },
    WriteDialog {
        addr: u16,
        tab_index: usize,
        input: String,
        error: Option<String>,
    },
    LabelDialog {
        addr: u16,
        tab_index: usize,
        input: String,
    },
    CommandBar {
        input: String,
        error: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AddrFormat {
    #[default]
    Hex,
    Decimal,
}

#[derive(Debug, Clone, Default)]
pub struct PaneState {
    pub scroll_offset: usize,
    pub selected_row: usize,
    pub addr_format: AddrFormat,
    pub num_format: NumFormat,
}

#[derive(Debug)]
pub struct UiState {
    /// Index into `config.ranges` — which pane is active (has keyboard focus).
    pub active_tab: usize,
    pub focus: FocusPane,
    /// Per-pane scroll/selection state, indexed parallel to `config.ranges`.
    pub panes: Vec<PaneState>,
    pub log_scroll: usize,
    pub input_mode: InputMode,
}

impl UiState {
    pub fn new(num_panes: usize) -> Self {
        Self {
            active_tab: 0,
            focus: FocusPane::Registers,
            panes: vec![PaneState::default(); num_panes],
            log_scroll: 0,
            input_mode: InputMode::Normal,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AppConfig, RegisterType};
    use std::path::PathBuf;

    fn test_config_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/test_config.json")
    }

    #[test]
    fn loads_test_config_json() {
        let cfg = AppConfig::load(&test_config_path()).expect("config parses");
        assert_eq!(cfg.host, "10.203.11.93");
        assert_eq!(cfg.port, 502);
        assert_eq!(cfg.unit, 1);
        assert_eq!(cfg.ranges.len(), 7);
        assert!(cfg.decimal_addresses);

        let types: Vec<_> = cfg.ranges.iter().map(|r| r.reg_type).collect();
        assert!(types.contains(&RegisterType::HoldingRegisters));
        assert!(types.contains(&RegisterType::Coils));
        assert!(types.contains(&RegisterType::DiscreteInputs));
        assert!(types.contains(&RegisterType::InputRegisters));
    }

    #[test]
    fn apply_range_defaults_prepopulates_all_addresses() {
        let cfg = AppConfig::load(&test_config_path()).unwrap();
        let mut state = AppState::new(cfg);
        state.apply_range_defaults();

        for (i, range) in state.config.ranges.iter().enumerate() {
            let regs = &state.registers[i];
            assert_eq!(
                regs.len(),
                range.count as usize,
                "range[{i}] ({}) should have {} entries, got {}",
                range.reg_type,
                range.count,
                regs.len()
            );
            for offset in 0..range.count {
                let addr = range.start + offset;
                assert!(
                    regs.contains_key(&addr),
                    "range[{i}] missing address {addr}"
                );
            }
        }
    }

    #[test]
    fn apply_range_defaults_assigns_labels() {
        let cfg = AppConfig::load(&test_config_path()).unwrap();
        let mut state = AppState::new(cfg);
        state.apply_range_defaults();

        assert_eq!(
            state.registers[0].get(&40).unwrap().label.as_deref(),
            Some("SOH [%]")
        );
        assert_eq!(
            state.registers[3].get(&62).unwrap().label.as_deref(),
            Some("Q_Act [VAr]")
        );
        assert_eq!(
            state.registers[4].get(&3).unwrap().label.as_deref(),
            Some("Relay 4")
        );
        assert!(state.registers[4].get(&1).unwrap().label.is_none());
    }

    /// Regression test for the render-side panic:
    /// `chunks(format_width)` must never produce a short trailing chunk.
    #[test]
    fn register_map_length_divisible_by_format_width() {
        let cfg = AppConfig::load(&test_config_path()).unwrap();
        let mut state = AppState::new(cfg);
        state.apply_range_defaults();

        for (i, range) in state.config.ranges.iter().enumerate() {
            if range.reg_type.is_coil_type() {
                continue;
            }
            let width = state.ui.panes[i].num_format.width();
            let len = state.registers[i].len();
            assert_eq!(
                len % width,
                0,
                "range[{i}] ({}) len={len} not divisible by width={width}",
                range.reg_type
            );
        }
    }

    #[test]
    fn apply_range_defaults_sets_pane_format_from_config() {
        let cfg = AppConfig::load(&test_config_path()).unwrap();
        let mut state = AppState::new(cfg);
        state.apply_range_defaults();

        assert_eq!(state.ui.panes[0].num_format, NumFormat::Int16);
        assert_eq!(state.ui.panes[2].num_format, NumFormat::Int32);
        assert_eq!(state.ui.panes[3].num_format, NumFormat::Float32);
        assert_eq!(state.ui.panes[6].num_format, NumFormat::Float64);
    }

    /// `:save` roundtrip: load → apply defaults → save → the emitted JSON
    /// must be structurally equal to the original file. Structural (not
    /// textual) comparison via `serde_json::Value` ignores formatting and
    /// object key order.
    #[test]
    fn save_config_roundtrip_matches_original_json() {
        let path = test_config_path();
        let original_text = std::fs::read_to_string(&path).expect("read test config");
        let original: serde_json::Value =
            serde_json::from_str(&original_text).expect("original parses as JSON");

        let cfg = AppConfig::load(&path).unwrap();
        let mut state = AppState::new(cfg);
        state.apply_range_defaults();

        let saved_cfg = state.build_saved_config();
        let saved_text = serde_json::to_string_pretty(&saved_cfg).expect("serialize");
        let saved: serde_json::Value =
            serde_json::from_str(&saved_text).expect("saved parses as JSON");

        assert_eq!(
            original, saved,
            "save roundtrip differs from original:\n--- original ---\n{original:#}\n--- saved ---\n{saved:#}"
        );
    }

    /// If the user edits a label in-memory, the saved config must reflect
    /// that change (and only that change) relative to the original.
    #[test]
    fn save_config_captures_edited_label() {
        let path = test_config_path();
        let cfg = AppConfig::load(&path).unwrap();
        let mut state = AppState::new(cfg);
        state.apply_range_defaults();

        state.registers[0].get_mut(&40).unwrap().label = Some("NEW_SOH".into());

        let saved_cfg = state.build_saved_config();
        let saved = serde_json::to_value(&saved_cfg).unwrap();

        let new_label = &saved["ranges"][0]["labels"]["40"];
        assert_eq!(new_label.as_str(), Some("NEW_SOH"));

        // Other ranges' labels are untouched.
        let untouched = &saved["ranges"][3]["labels"]["58"];
        assert_eq!(untouched.as_str(), Some("Freq [Hz]"));
    }
}
