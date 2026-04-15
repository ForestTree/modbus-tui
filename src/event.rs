use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::{AddrFormat, AppState, FocusPane, InputMode, WriteRequest};
use crate::config::Mode;
use crate::format::NumFormat;

/// Handle a single key event.
pub fn handle_key(state: &mut AppState, key: KeyEvent) {
    match &state.ui.input_mode {
        InputMode::Normal => handle_normal(state, key),
        InputMode::HelpDialog => handle_help_dialog(state, key),
        InputMode::FormatDialog { .. } => handle_format_dialog(state, key),
        InputMode::WriteDialog { .. } => handle_write_dialog(state, key),
        InputMode::LabelDialog { .. } => handle_label_dialog(state, key),
        InputMode::CommandBar { .. } => handle_command_bar(state, key),
    }
}

// ---------------------------------------------------------------------------
// Normal mode
// ---------------------------------------------------------------------------

fn handle_normal(state: &mut AppState, key: KeyEvent) {
    let tab_count = state.tab_count();

    match key.code {
        KeyCode::Char('q') | KeyCode::Esc => {
            state.running = false;
        }
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            state.running = false;
        }

        // Switch active pane — cycle through visible register panes
        KeyCode::Tab if tab_count > 0 => {
            state.ui.active_tab = (state.ui.active_tab + 1) % tab_count;
        }
        KeyCode::BackTab if tab_count > 0 => {
            state.ui.active_tab = (state.ui.active_tab + tab_count - 1) % tab_count;
        }

        // Focus toggle
        KeyCode::F(2) => {
            state.ui.focus = match state.ui.focus {
                FocusPane::Registers => FocusPane::Log,
                FocusPane::Log => FocusPane::Registers,
            };
        }

        // Write dialog (writable register types; server mode allows all types)
        KeyCode::Char('w') => {
            if state.active_tab_is_writable()
                && let Some(addr) = state.selected_addr()
            {
                state.ui.input_mode = InputMode::WriteDialog {
                    addr,
                    tab_index: state.ui.active_tab,
                    input: String::new(),
                    error: None,
                };
            }
        }

        // Help dialog
        KeyCode::F(1) => {
            state.ui.input_mode = InputMode::HelpDialog;
        }

        // Command bar
        KeyCode::Char(':') => {
            state.ui.input_mode = InputMode::CommandBar {
                input: String::new(),
                error: None,
            };
        }

        // Address format: d/D = decimal, h/H = hex (lowercase=active pane, uppercase=all)
        KeyCode::Char('d') => {
            if let Some(p) = state.ui.panes.get_mut(state.ui.active_tab) {
                p.addr_format = AddrFormat::Decimal;
            }
        }
        KeyCode::Char('D') => {
            for p in &mut state.ui.panes {
                p.addr_format = AddrFormat::Decimal;
            }
        }
        KeyCode::Char('h') => {
            if let Some(p) = state.ui.panes.get_mut(state.ui.active_tab) {
                p.addr_format = AddrFormat::Hex;
            }
        }
        KeyCode::Char('H') => {
            for p in &mut state.ui.panes {
                p.addr_format = AddrFormat::Hex;
            }
        }

        // Format dialog (only for word register panes, not coils)
        KeyCode::Char('f') => {
            if !state.active_tab_is_coils() && state.tab_count() > 0 {
                let current = state
                    .ui
                    .panes
                    .get(state.ui.active_tab)
                    .map(|p| p.num_format)
                    .unwrap_or_default();
                let sel = NumFormat::ALL
                    .iter()
                    .position(|f| *f == current)
                    .unwrap_or(0);
                state.ui.input_mode = InputMode::FormatDialog { selected: sel };
            }
        }

        // Label dialog — edit label for the selected register
        KeyCode::Char('l') => {
            if state.tab_count() > 0
                && let Some(addr) = state.selected_addr()
            {
                let existing = state
                    .registers
                    .get(state.ui.active_tab)
                    .and_then(|m| m.get(&addr))
                    .and_then(|rv| rv.label.clone())
                    .unwrap_or_default();
                state.ui.input_mode = InputMode::LabelDialog {
                    addr,
                    tab_index: state.ui.active_tab,
                    input: existing,
                };
            }
        }

        // Scroll / selection — operates on active pane or log
        KeyCode::Up | KeyCode::Char('k') => match state.ui.focus {
            FocusPane::Registers => {
                if let Some(p) = state.ui.panes.get_mut(state.ui.active_tab) {
                    p.selected_row = p.selected_row.saturating_sub(1);
                }
            }
            FocusPane::Log => {
                state.ui.log_scroll = state.ui.log_scroll.saturating_add(1);
            }
        },
        KeyCode::Down | KeyCode::Char('j') => match state.ui.focus {
            FocusPane::Registers => {
                if let Some(p) = state.ui.panes.get_mut(state.ui.active_tab) {
                    p.selected_row = p.selected_row.saturating_add(1);
                }
            }
            FocusPane::Log => {
                state.ui.log_scroll = state.ui.log_scroll.saturating_sub(1);
            }
        },
        KeyCode::PageUp => match state.ui.focus {
            FocusPane::Registers => {
                if let Some(p) = state.ui.panes.get_mut(state.ui.active_tab) {
                    p.selected_row = p.selected_row.saturating_sub(20);
                }
            }
            FocusPane::Log => {
                state.ui.log_scroll = state.ui.log_scroll.saturating_add(20);
            }
        },
        KeyCode::PageDown => match state.ui.focus {
            FocusPane::Registers => {
                if let Some(p) = state.ui.panes.get_mut(state.ui.active_tab) {
                    p.selected_row = p.selected_row.saturating_add(20);
                }
            }
            FocusPane::Log => {
                state.ui.log_scroll = state.ui.log_scroll.saturating_sub(20);
            }
        },
        KeyCode::Home => match state.ui.focus {
            FocusPane::Registers => {
                if let Some(p) = state.ui.panes.get_mut(state.ui.active_tab) {
                    p.selected_row = 0;
                    p.scroll_offset = 0;
                }
            }
            FocusPane::Log => {
                state.ui.log_scroll = state.log.entries.len();
            }
        },
        KeyCode::End => match state.ui.focus {
            FocusPane::Registers => {}
            FocusPane::Log => {
                state.ui.log_scroll = 0;
            }
        },

        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Help dialog — any key dismisses
// ---------------------------------------------------------------------------

fn handle_help_dialog(state: &mut AppState, _key: KeyEvent) {
    state.ui.input_mode = InputMode::Normal;
}

// ---------------------------------------------------------------------------
// Format dialog — select numeric format for active pane
// ---------------------------------------------------------------------------

fn handle_format_dialog(state: &mut AppState, key: KeyEvent) {
    let selected = match state.ui.input_mode {
        InputMode::FormatDialog { selected } => selected,
        _ => return,
    };
    let count = NumFormat::ALL.len();

    match key.code {
        KeyCode::Esc => {
            state.ui.input_mode = InputMode::Normal;
        }
        KeyCode::Up | KeyCode::Char('k') => {
            state.ui.input_mode = InputMode::FormatDialog {
                selected: (selected + count - 1) % count,
            };
        }
        KeyCode::Down | KeyCode::Char('j') => {
            state.ui.input_mode = InputMode::FormatDialog {
                selected: (selected + 1) % count,
            };
        }
        KeyCode::Enter => {
            if let Some(p) = state.ui.panes.get_mut(state.ui.active_tab) {
                p.num_format = NumFormat::ALL[selected];
                p.selected_row = 0;
            }
            state.ui.input_mode = InputMode::Normal;
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Label dialog mode
// ---------------------------------------------------------------------------

fn handle_label_dialog(state: &mut AppState, key: KeyEvent) {
    let (addr, tab_index, mut input) = match state.ui.input_mode.clone() {
        InputMode::LabelDialog {
            addr,
            tab_index,
            input,
        } => (addr, tab_index, input),
        _ => return,
    };

    match key.code {
        KeyCode::Esc => {
            state.ui.input_mode = InputMode::Normal;
        }
        KeyCode::Enter => {
            if let Some(map) = state.registers.get_mut(tab_index)
                && let Some(rv) = map.get_mut(&addr)
            {
                rv.label = if input.is_empty() { None } else { Some(input) };
            }
            state.ui.input_mode = InputMode::Normal;
        }
        KeyCode::Backspace => {
            input.pop();
            state.ui.input_mode = InputMode::LabelDialog {
                addr,
                tab_index,
                input,
            };
        }
        KeyCode::Char(c) => {
            input.push(c);
            state.ui.input_mode = InputMode::LabelDialog {
                addr,
                tab_index,
                input,
            };
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Write dialog mode
// ---------------------------------------------------------------------------

fn handle_write_dialog(state: &mut AppState, key: KeyEvent) {
    let (addr, tab_index, mut input, _error) = match state.ui.input_mode.clone() {
        InputMode::WriteDialog {
            addr,
            tab_index,
            input,
            error,
        } => (addr, tab_index, input, error),
        _ => return,
    };

    // Get the active pane's numeric format (default Int16 for coils)
    let nf = if state.active_tab_is_coils() {
        NumFormat::Uint16
    } else {
        state
            .ui
            .panes
            .get(tab_index)
            .map(|p| p.num_format)
            .unwrap_or_default()
    };

    match key.code {
        KeyCode::Esc => {
            state.ui.input_mode = InputMode::Normal;
        }
        KeyCode::Enter => {
            let ws = crate::format::WordSwap {
                ints: state.config.swap_ints,
                floats: state.config.swap_floats,
                words: state.config.swap_words,
                bytes: state.config.swap_bytes,
            };
            match nf.parse_value(&input, &ws) {
                Ok(values) => {
                    let vals_display: Vec<String> =
                        values.iter().map(|v| format!("0x{:04X}", v)).collect();
                    if state.config.mode == Mode::Server {
                        // Server mode: write directly to RegisterStore
                        if let Some(ref store) = state.server_store {
                            let reg_type = state.config.ranges[tab_index].reg_type;
                            crate::modbus::server::write_to_store(store, reg_type, addr, &values);
                            state.log.info(format!(
                                "write: addr=0x{:04X} [{}] ({})",
                                addr,
                                input.trim(),
                                vals_display.join(", ")
                            ));
                        }
                    } else if let Some(ref tx) = state.write_tx {
                        let req = WriteRequest {
                            tab_index,
                            addr,
                            values,
                        };
                        if tx.send(req).is_ok() {
                            state.log.info(format!(
                                "write request: addr=0x{:04X} [{}] ({})",
                                addr,
                                input.trim(),
                                vals_display.join(", ")
                            ));
                        }
                    }
                    state.ui.input_mode = InputMode::Normal;
                }
                Err(e) => {
                    state.ui.input_mode = InputMode::WriteDialog {
                        addr,
                        tab_index,
                        input,
                        error: Some(e),
                    };
                }
            }
        }
        KeyCode::Backspace => {
            input.pop();
            state.ui.input_mode = InputMode::WriteDialog {
                addr,
                tab_index,
                input,
                error: None,
            };
        }
        KeyCode::Char(c) => {
            input.push(c);
            state.ui.input_mode = InputMode::WriteDialog {
                addr,
                tab_index,
                input,
                error: None,
            };
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Command bar mode
// ---------------------------------------------------------------------------

fn handle_command_bar(state: &mut AppState, key: KeyEvent) {
    let (mut input, _error) = match state.ui.input_mode.clone() {
        InputMode::CommandBar { input, error } => (input, error),
        _ => return,
    };

    match key.code {
        KeyCode::Esc => {
            state.ui.input_mode = InputMode::Normal;
        }
        KeyCode::Enter => {
            let cmd = input.trim().to_string();
            execute_command(state, &cmd);
            state.ui.input_mode = InputMode::Normal;
        }
        KeyCode::Backspace => {
            input.pop();
            state.ui.input_mode = InputMode::CommandBar { input, error: None };
        }
        KeyCode::Char(c) => {
            input.push(c);
            state.ui.input_mode = InputMode::CommandBar { input, error: None };
        }
        _ => {}
    }
}

fn execute_command(state: &mut AppState, cmd: &str) {
    let parts: Vec<&str> = cmd.split_whitespace().collect();
    if parts.is_empty() {
        return;
    }

    match parts[0] {
        "poll" => {
            if parts.len() < 2 {
                state.log.error("usage: poll <interval_ms>");
                return;
            }
            match parts[1].parse::<u64>() {
                Ok(ms) if (10..=60_000).contains(&ms) => {
                    state.config.poll_interval_ms = ms;
                    state.log.info(format!("poll interval changed to {ms} ms"));
                }
                Ok(ms) => {
                    state
                        .log
                        .error(format!("poll interval {ms} out of range (10..60000)"));
                }
                Err(e) => {
                    state.log.error(format!("invalid poll interval: {e}"));
                }
            }
        }
        "save" => {
            let path = if parts.len() > 1 {
                let p = parts[1];
                if p.ends_with(".json") {
                    p.to_string()
                } else {
                    format!("{p}.json")
                }
            } else {
                "config.json".to_string()
            };
            save_config(state, &path);
        }
        "export" => {
            let path = if parts.len() > 1 {
                let p = parts[1];
                if p.ends_with(".json") {
                    p.to_string()
                } else {
                    format!("{p}.json")
                }
            } else {
                "registers.json".to_string()
            };
            export_registers(state, &path);
        }
        _ => {
            state.log.warn(format!("unknown command: {}", parts[0]));
        }
    }
}

fn export_registers(state: &mut AppState, path: &str) {
    let sr = state.config.start_reference;
    let ws = crate::format::WordSwap {
        ints: state.config.swap_ints,
        floats: state.config.swap_floats,
        words: state.config.swap_words,
        bytes: state.config.swap_bytes,
    };

    let mut sections = Vec::new();

    for (i, range) in state.config.ranges.iter().enumerate() {
        let regs = &state.registers[i];
        if regs.is_empty() {
            continue;
        }
        let pane = state.ui.panes.get(i);
        let addr_fmt = pane.map(|p| p.addr_format).unwrap_or_default();
        let nf = pane.map(|p| p.num_format).unwrap_or_default();
        let section_name = range.tab_label(sr, addr_fmt);
        let is_coils = range.reg_type.is_coil_type();

        let format_addr = |addr: u16| -> String {
            match addr_fmt {
                AddrFormat::Hex => format!("0x{:04X}", addr),
                AddrFormat::Decimal => format!("{}", addr),
            }
        };

        let mut rows = Vec::new();

        if is_coils {
            for (&addr, rv) in regs.iter() {
                let mut entry = serde_json::Map::new();
                entry.insert(
                    "address".into(),
                    serde_json::Value::String(format_addr(addr + sr)),
                );
                entry.insert(
                    "value".into(),
                    serde_json::Value::String(if rv.raw != 0 { "ON" } else { "OFF" }.into()),
                );
                entry.insert(
                    "timestamp".into(),
                    serde_json::Value::String(rv.changed_wall.clone()),
                );
                if let Some(label) = &rv.label {
                    entry.insert("label".into(), serde_json::Value::String(label.clone()));
                }
                rows.push(serde_json::Value::Object(entry));
            }
        } else {
            let width = nf.width();
            let addrs: Vec<u16> = regs.keys().copied().collect();
            for chunk in addrs.chunks(width) {
                let base_addr = chunk[0];
                let vals: Vec<u16> = chunk
                    .iter()
                    .map(|a| regs.get(a).map(|rv| rv.raw).unwrap_or(0))
                    .collect();

                let base_rv = regs.get(&base_addr).unwrap();
                let hex_str = vals
                    .iter()
                    .map(|v| format!("{:04X}", v))
                    .collect::<Vec<_>>()
                    .join(" ");
                let value_str = nf.format_value(&vals, &ws);

                let mut entry = serde_json::Map::new();
                entry.insert(
                    "address".into(),
                    serde_json::Value::String(format_addr(base_addr + sr)),
                );
                entry.insert("hex".into(), serde_json::Value::String(hex_str));
                entry.insert("value".into(), serde_json::Value::String(value_str));
                entry.insert(
                    "timestamp".into(),
                    serde_json::Value::String(base_rv.changed_wall.clone()),
                );
                if let Some(label) = &base_rv.label {
                    entry.insert("label".into(), serde_json::Value::String(label.clone()));
                }
                rows.push(serde_json::Value::Object(entry));
            }
        }

        let mut section = serde_json::Map::new();
        if !is_coils {
            section.insert(
                "format".into(),
                serde_json::Value::String(nf.column_header().into()),
            );
        }
        section.insert("registers".into(), serde_json::Value::Array(rows));
        sections.push((section_name, serde_json::Value::Object(section)));
    }

    // Use ordered map to preserve pane order
    let out: serde_json::Map<String, serde_json::Value> = sections.into_iter().collect();
    match serde_json::to_string_pretty(&serde_json::Value::Object(out)) {
        Ok(json) => match std::fs::write(path, &json) {
            Ok(()) => state.log.info(format!("exported registers to {path}")),
            Err(e) => state.log.error(format!("export failed: {e}")),
        },
        Err(e) => state.log.error(format!("export serialization failed: {e}")),
    }
}

fn save_config(state: &mut AppState, path: &str) {
    let config = state.build_saved_config();
    match serde_json::to_string_pretty(&config) {
        Ok(json) => match std::fs::write(path, &json) {
            Ok(()) => state.log.info(format!("config saved to {path}")),
            Err(e) => state.log.error(format!("save failed: {e}")),
        },
        Err(e) => state.log.error(format!("save serialization failed: {e}")),
    }
}
