use std::collections::{BTreeMap, VecDeque};
use std::fmt;
use std::sync::Arc;
use std::time::Instant;

use chrono::Local;
use tokio::sync::Mutex;

use crate::config::{AppConfig, };
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
            config,
        }
    }

    /// Number of tabs (= number of configured ranges).
    pub fn tab_count(&self) -> usize {
        self.config.ranges.len()
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
    pub fn active_tab_is_writable(&self) -> bool {
        self.config
            .ranges
            .get(self.ui.active_tab)
            .map(|r| r.reg_type.is_writable())
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
}

impl RegisterValue {
    pub fn new(raw: u16) -> Self {
        Self {
            raw,
            last_read: Instant::now(),
            label: None,
            changed_at: Some(Instant::now()),
            prev_raw: None,
            changed_wall: Local::now().format("%Y-%m-%d %H:%M:%S%.3f").to_string(),
        }
    }

    pub fn update(&mut self, raw: u16) {
        if raw != self.raw {
            self.prev_raw = Some(self.raw);
            self.changed_at = Some(Instant::now());
            self.changed_wall = Local::now().format("%Y-%m-%d %H:%M:%S%.3f").to_string();
        }
        self.raw = raw;
        self.last_read = Instant::now();
    }

    pub fn recently_changed(&self) -> bool {
        self.changed_at
            .map(|t| t.elapsed().as_secs() < CHANGE_HIGHLIGHT_SECS)
            .unwrap_or(false)
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
}

impl fmt::Display for LogLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Info => write!(f, "INFO"),
            Self::Warn => write!(f, "WARN"),
            Self::Error => write!(f, "ERR "),
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

#[derive(Debug, Clone)]
pub struct PaneState {
    pub scroll_offset: usize,
    pub selected_row: usize,
    pub addr_format: AddrFormat,
    pub num_format: NumFormat,
}

impl Default for PaneState {
    fn default() -> Self {
        Self {
            scroll_offset: 0,
            selected_row: 0,
            addr_format: AddrFormat::default(),
            num_format: NumFormat::default(),
        }
    }
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
