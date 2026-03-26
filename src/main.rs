mod app;
mod config;
mod event;
mod format;
mod modbus;
mod ui;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const COPYRIGHT: &str = "(c) 2026 ForestTree";

use std::io;
use std::time::Duration;

use anyhow::Result;
use clap::Parser;
use crossterm::event::{Event, EventStream};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use futures_util::StreamExt;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use crate::app::new_shared_state;
use crate::config::{AppConfig, Cli, Mode};
use crate::ui::render;

const TICK: Duration = Duration::from_millis(100);

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let decimal_addresses = cli.decimal_addresses;
    let config = AppConfig::from_cli(&cli)?;
    let (state, shutdown, write_tx, write_rx) = new_shared_state(config);

    // Apply startup overrides to pane states (from CLI flags and config)
    {
        let mut s = state.lock().await;
        // -D flag or config: set all panes to decimal address format
        if decimal_addresses || s.config.decimal_addresses {
            for p in &mut s.ui.panes {
                p.addr_format = app::AddrFormat::Decimal;
            }
        }
        // Apply per-range initial numeric format from START:COUNT:FMT or config
        let formats: Vec<_> = s.config.ranges.iter().map(|r| r.initial_format).collect();
        for (i, fmt) in formats.into_iter().enumerate() {
            if let Some(nf) = fmt
                && let Some(p) = s.ui.panes.get_mut(i)
            {
                p.num_format = nf;
            }
        }
        // Apply per-register labels from config
        let all_labels: Vec<_> = s.config.ranges.iter().map(|r| r.labels.clone()).collect();
        for (i, labels) in all_labels.into_iter().enumerate() {
            for (addr, label) in labels {
                let rv = s.registers[i]
                    .entry(addr)
                    .or_insert_with(|| app::RegisterValue::new(0));
                rv.label = Some(label);
            }
        }
    }

    // Log startup
    {
        let mut s = state.lock().await;
        s.log.info("modbus-tui started");
        let target = format!(
            "target {}:{} unit={}",
            s.config.host, s.config.port, s.config.unit,
        );
        s.log.info(target);
    }

    // --- spawn modbus task ---
    let mode = state.lock().await.config.mode;
    match mode {
        Mode::Client => {
            // Store write_tx in app state so the UI can send write requests
            state.lock().await.write_tx = Some(write_tx);
            modbus::client::spawn(state.clone(), write_rx);
        }
        Mode::Server => {
            let store = modbus::server::spawn(state.clone(), shutdown.subscribe());
            state.lock().await.server_store = Some(store);
            // write channel unused in server mode — drop it
            drop(write_tx);
            drop(write_rx);
        }
    }

    // --- terminal setup ---
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // --- main loop ---
    let result = run_loop(&mut terminal, &state).await;

    // --- teardown (always runs) ---
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    // Signal background tasks to stop.
    let _ = shutdown.send(true);

    result
}

async fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    state: &app::SharedState,
) -> Result<()> {
    let mut events = EventStream::new();
    loop {
        // Draw
        {
            let mut s = state.lock().await;
            // In server mode, sync RegisterStore → UI registers
            if let Some(store) = s.server_store.clone() {
                modbus::server::sync_store_to_registers(&store, &mut s);
            }
            terminal.draw(|frame| render::draw(frame, &s))?;
        }

        // Wait for input or tick — fully async, no thread blocking
        let event = tokio::select! {
            _ = tokio::time::sleep(TICK) => None,
            ev = events.next() => match ev {
                Some(Ok(e)) => Some(e),
                _ => None,
            },
        };

        // On Windows, crossterm emits both Press and Release events for each
        // keystroke (macOS/Linux only emit Press). Filter to Press only so that
        // every key handler runs exactly once on all platforms.
        if let Some(Event::Key(key)) = event.filter(
            |e| matches!(e, Event::Key(k) if k.kind == crossterm::event::KeyEventKind::Press),
        ) {
            let mut s = state.lock().await;
            event::handle_key(&mut s, key);
            if !s.running {
                break;
            }
        }
    }
    Ok(())
}
