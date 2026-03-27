use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Clear, List, ListItem, Paragraph, Row, Table};

use crate::app::{
    AppState, ConnectionStatus, FocusPane, InputMode, LogLevel, RegisterValue, ServerStats,
};
use crate::config::Mode;
use crate::format::NumFormat;

pub fn draw(frame: &mut Frame, state: &AppState) {
    let has_bottom_bar = !matches!(state.ui.input_mode, InputMode::Normal);
    let bottom_height = if has_bottom_bar { 1 } else { 0 };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(8),
            Constraint::Length(bottom_height),
        ])
        .split(frame.area());

    draw_status_bar(frame, state, chunks[0]);
    draw_content(frame, state, chunks[1]);
    draw_log(frame, state, chunks[2]);

    if has_bottom_bar {
        draw_bottom_bar(frame, state, chunks[3]);
    }

    // Overlay dialogs
    match &state.ui.input_mode {
        InputMode::WriteDialog { .. } => draw_write_dialog(frame, state),
        InputMode::LabelDialog { .. } => draw_label_dialog(frame, state),
        InputMode::HelpDialog => draw_help_dialog(frame),
        InputMode::FormatDialog { selected } => draw_format_dialog(frame, *selected),
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Status bar
// ---------------------------------------------------------------------------

fn draw_status_bar(frame: &mut Frame, state: &AppState, area: Rect) {
    let mode_str = format!("{:?}", state.config.mode);
    let target_str = format!("{}:{}", state.config.host, state.config.port);
    let unit_str = format!("unit={}", state.config.unit);

    let is_server = state.config.mode == Mode::Server;

    let (conn_text, conn_color) = match &state.connection {
        ConnectionStatus::Disconnected => ("Disconnected", Color::Gray),
        ConnectionStatus::Connecting => ("Connecting…", Color::Yellow),
        ConnectionStatus::Connected => {
            if is_server {
                ("Bound", Color::Green)
            } else {
                ("Connected", Color::Green)
            }
        }
        ConnectionStatus::Error(_) => ("", Color::Red),
    };

    let conn_span = if let ConnectionStatus::Error(e) = &state.connection {
        Span::styled(
            format!("Error: {e}"),
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled(
            conn_text,
            Style::default().fg(conn_color).add_modifier(Modifier::BOLD),
        )
    };

    let sep = Span::styled(" | ", Style::default().fg(Color::DarkGray));

    let mut spans = vec![
        Span::styled(
            format!(" {mode_str} "),
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(target_str, Style::default().fg(Color::White)),
        sep.clone(),
        Span::styled(unit_str, Style::default().fg(Color::DarkGray)),
        sep.clone(),
        conn_span,
    ];

    if !is_server {
        const SPINNER_CHARS: &[char] = &['|', '/', '-', '\\'];
        let spinner = SPINNER_CHARS[(state.spinner_tick as usize) % SPINNER_CHARS.len()];
        spans.push(Span::styled(
            format!(" {spinner} "),
            Style::default().fg(Color::DarkGray),
        ));
        spans.push(Span::styled(
            format!("poll={}ms", state.config.poll_interval_ms),
            Style::default().fg(Color::DarkGray),
        ));
    }

    spans.push(sep.clone());
    spans.push(Span::styled(
        if state.config.start_reference == 0 {
            "0-based addressing"
        } else {
            "1-based addressing"
        },
        Style::default().fg(Color::DarkGray),
    ));

    // Swap indicator — compact labels: BS = byte swap, WS = word swap
    let mut swap_parts = Vec::new();
    if state.config.swap_bytes {
        swap_parts.push("BS: all");
    }
    match (state.config.swap_ints, state.config.swap_floats) {
        (true, true) => swap_parts.push("WS: ints+floats"),
        (true, false) => swap_parts.push("WS: ints"),
        (false, true) => swap_parts.push("WS: floats"),
        (false, false) => {}
    }
    if !swap_parts.is_empty() {
        spans.push(sep);
        spans.push(Span::styled(
            format!("swap [{}]", swap_parts.join(", ")),
            Style::default().fg(Color::Yellow),
        ));
    }

    let status_line = Line::from(spans);

    let block = Block::default().borders(Borders::ALL).title(Span::styled(
        format!(" modbus-tui v{} {} ", crate::VERSION, crate::COPYRIGHT),
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    ));

    frame.render_widget(Paragraph::new(status_line).block(block), area);
}

// ---------------------------------------------------------------------------
// Main content
// ---------------------------------------------------------------------------

fn draw_content(frame: &mut Frame, state: &AppState, area: Rect) {
    match state.config.mode {
        Mode::Server => {
            if state.config.ranges.is_empty() {
                draw_server_content(frame, state, area);
            } else {
                // Register grid + server stats below
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Min(6), Constraint::Length(8)])
                    .split(area);
                draw_register_grid(frame, state, chunks[0]);
                draw_server_content(frame, state, chunks[1]);
            }
        }
        Mode::Client => {
            if state.config.ranges.is_empty() {
                let block = Block::default()
                    .borders(Borders::ALL)
                    .title(" No register ranges configured ");
                let msg = Paragraph::new("  Use --hr, --ir, --co, --di to add register ranges.")
                    .style(Style::default().fg(Color::DarkGray))
                    .block(block);
                frame.render_widget(msg, area);
                return;
            }
            draw_register_grid(frame, state, area);
        }
    }
}

/// Draw all register panes in a grid layout.
/// 1-2 ranges: 1 row, N columns.
/// 3-4 ranges: 2 rows, 1-2 columns each...
fn draw_register_grid(frame: &mut Frame, state: &AppState, area: Rect) {
    let n = state.config.ranges.len();
    let cols = if n <= 1 { 1 } else { 2 };
    let rows = n.div_ceil(cols); // ceil division

    // Split vertically into rows
    let row_constraints: Vec<Constraint> = (0..rows)
        .map(|_| Constraint::Ratio(1, rows as u32))
        .collect();
    let row_areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints(row_constraints)
        .split(area);

    for row in 0..rows {
        let start_idx = row * cols;
        let end_idx = (start_idx + cols).min(n);
        let panes_in_row = end_idx - start_idx;

        if panes_in_row == 0 {
            break;
        }

        let col_constraints: Vec<Constraint> = (0..panes_in_row)
            .map(|_| Constraint::Ratio(1, panes_in_row as u32))
            .collect();
        let col_areas = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(col_constraints)
            .split(row_areas[row]);

        for col in 0..panes_in_row {
            let pane_idx = start_idx + col;
            let is_active =
                pane_idx == state.ui.active_tab && state.ui.focus == FocusPane::Registers;
            draw_register_pane(frame, state, pane_idx, is_active, col_areas[col]);
        }
    }
}

// ---------------------------------------------------------------------------
// Server content (unchanged)
// ---------------------------------------------------------------------------

fn draw_server_content(frame: &mut Frame, state: &AppState, area: Rect) {
    let sub = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(area);

    draw_server_connections(frame, state, sub[0]);
    draw_server_requests(frame, &state.server, sub[1]);
}

fn draw_server_connections(frame: &mut Frame, state: &AppState, area: Rect) {
    let stats = &state.server;
    let lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled(
                "  Active connections: ",
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled(
                format!("{}", stats.active_connections),
                if stats.active_connections > 0 {
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                },
            ),
        ]),
        Line::from(vec![
            Span::styled(
                "  Total connections:  ",
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled(
                format!("{}", stats.total_connections),
                Style::default().fg(Color::White),
            ),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("  Bind address: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{}:{}", state.config.host, state.config.port),
                Style::default().fg(Color::Cyan),
            ),
        ]),
        Line::from(vec![
            Span::styled("  Unit ID:      ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{}", state.config.unit),
                Style::default().fg(Color::White),
            ),
        ]),
    ];
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(" Connections ");
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn draw_server_requests(frame: &mut Frame, stats: &ServerStats, area: Rect) {
    let total_reads = stats.requests_coils
        + stats.requests_discrete_inputs
        + stats.requests_holding_registers
        + stats.requests_input_registers;
    let total = total_reads + stats.requests_write + stats.requests_other;

    let rows_data = vec![
        ("Read Holding Registers", stats.requests_holding_registers),
        ("Read Input Registers", stats.requests_input_registers),
        ("Read Coils", stats.requests_coils),
        ("Read Discrete Inputs", stats.requests_discrete_inputs),
        ("Write (all)", stats.requests_write),
        ("Other / Unsupported", stats.requests_other),
    ];

    let header = Row::new(vec![
        Cell::from("Request Type").style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("Count").style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
    ])
    .height(1);

    let table_rows: Vec<Row> = rows_data
        .into_iter()
        .map(|(name, count)| {
            let s = if count > 0 {
                Style::default().fg(Color::Green)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            Row::new(vec![
                Cell::from(name),
                Cell::from(format!("{count}")).style(s),
            ])
        })
        .collect();

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(format!(" Requests ({total}) "));
    let widths = vec![Constraint::Min(26), Constraint::Length(12)];
    let table = Table::new(table_rows, &widths)
        .header(header)
        .block(block)
        .column_spacing(2);
    frame.render_widget(table, area);
}

// ---------------------------------------------------------------------------
// Dynamic tab row — one tab per configured range
// ---------------------------------------------------------------------------

/// Draw a single register pane at the given index.
fn draw_register_pane(
    frame: &mut Frame,
    state: &AppState,
    pane_idx: usize,
    is_active: bool,
    area: Rect,
) {
    let range = &state.config.ranges[pane_idx];
    let regs = &state.registers[pane_idx];
    let is_coils = range.reg_type.is_coil_type();
    let pane_state = &state.ui.panes[pane_idx];

    let border_style = if is_active {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(format!(
            " {} ",
            range.tab_label(state.config.start_reference, pane_state.addr_format)
        ));

    if regs.is_empty() {
        let msg = Paragraph::new("  Waiting…")
            .style(Style::default().fg(Color::DarkGray))
            .block(block);
        frame.render_widget(msg, area);
        return;
    }

    let sr = state.config.start_reference;
    let addr_fmt = pane_state.addr_format;
    let nf = pane_state.num_format;

    let (header, rows, widths, display_row_count) = if is_coils {
        // Simplified layout for coils / discrete inputs: Addr, Value, Timestamp, Label
        let hdr = Row::new(
            ["Addr", "Value", "Timestamp", "Label"]
                .into_iter()
                .map(|h| {
                    Cell::from(h).style(
                        Style::default()
                            .add_modifier(Modifier::BOLD)
                            .fg(Color::Cyan),
                    )
                }),
        )
        .height(1);

        let row_count = regs.len();
        let max_sel = row_count.saturating_sub(1);
        let selected = pane_state.selected_row.min(max_sel);

        let rs: Vec<Row> = regs
            .iter()
            .enumerate()
            .map(|(i, (addr, rv))| {
                build_coil_row(*addr + sr, rv, i == selected && is_active, addr_fmt)
            })
            .collect();

        let w = vec![
            Constraint::Length(8),
            Constraint::Length(5),
            Constraint::Length(23),
            Constraint::Min(5),
        ];
        (hdr, rs, w, row_count)
    } else {
        // Word registers: group by num_format width
        let width = nf.width();
        let value_header = nf.column_header();
        let hide_hex = state.config.hide_hex;
        let headers: Vec<&str> = if hide_hex {
            vec!["Addr", value_header, "Timestamp", "Label"]
        } else {
            vec!["Addr", "Hex", value_header, "Timestamp", "Label"]
        };
        let hdr = Row::new(headers.into_iter().map(|h| {
            Cell::from(h).style(
                Style::default()
                    .add_modifier(Modifier::BOLD)
                    .fg(Color::Cyan),
            )
        }))
        .height(1);

        // Collect addresses in order, then step by `width`
        let addrs: Vec<u16> = regs.keys().copied().collect();
        let grouped: Vec<(u16, Vec<u16>, Vec<u16>)> = addrs
            .chunks(width)
            .map(|chunk| {
                let base = chunk[0];
                let vals: Vec<u16> = chunk
                    .iter()
                    .map(|a| regs.get(a).map(|rv| rv.raw).unwrap_or(0))
                    .collect();
                (base, vals, chunk.to_vec())
            })
            .collect();

        let display_count = grouped.len();
        let max_sel = display_count.saturating_sub(1);
        let selected = pane_state.selected_row.min(max_sel);

        let rs: Vec<Row> = grouped
            .iter()
            .enumerate()
            .map(|(i, (base_addr, vals, chunk_addrs))| {
                let base_rv = regs.get(base_addr).unwrap();
                // For multi-register values, find the most recently changed
                // register in the group so the row highlights when ANY
                // sub-register changes.
                let rv = if width > 1 {
                    chunk_addrs
                        .iter()
                        .filter_map(|a| regs.get(a))
                        .filter(|r| r.recently_changed())
                        .max_by_key(|r| r.changed_at)
                        .unwrap_or(base_rv)
                } else {
                    base_rv
                };
                let hex_str = vals
                    .iter()
                    .map(|v| format!("{:04X}", v))
                    .collect::<Vec<_>>()
                    .join(" ");
                let ws = crate::format::WordSwap {
                    ints: state.config.swap_ints,
                    floats: state.config.swap_floats,
                    bytes: state.config.swap_bytes,
                };
                let value_str = nf.format_value(vals, &ws);
                build_word_row(
                    *base_addr + sr,
                    &hex_str,
                    &value_str,
                    rv,
                    base_rv.label.as_deref(),
                    i == selected && is_active,
                    addr_fmt,
                    hide_hex,
                )
            })
            .collect();

        // Value column width depends on actual format
        let val_width = match nf {
            NumFormat::Bin16 => 18u16,
            _ => match width {
                1 => 10,
                2 => 16,
                _ => 22,
            },
        };
        let w = if hide_hex {
            vec![
                Constraint::Length(8),
                Constraint::Length(val_width),
                Constraint::Length(23),
                Constraint::Min(5),
            ]
        } else {
            let hex_width = match width {
                1 => 8u16,
                2 => 12,
                _ => 22,
            };
            vec![
                Constraint::Length(8),
                Constraint::Length(hex_width),
                Constraint::Length(val_width),
                Constraint::Length(23),
                Constraint::Min(5),
            ]
        };
        (hdr, rs, w, display_count)
    };

    let max_sel = display_row_count.saturating_sub(1);
    let selected = pane_state.selected_row.min(max_sel);

    let table = Table::new(rows, &widths)
        .header(header)
        .block(block)
        .row_highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .column_spacing(1);

    let mut table_state = ratatui::widgets::TableState::default();
    if is_active {
        table_state.select(Some(selected));
    }
    frame.render_stateful_widget(table, area, &mut table_state);
}

/// Compute change-highlight color from a RegisterValue.
/// Values that are actively changing stay at full brightness Yellow.
fn change_color(rv: &RegisterValue) -> Color {
    if !rv.recently_changed() {
        return Color::Reset;
    }
    Color::Yellow
}

fn styles_for_row(rv: &RegisterValue, is_selected: bool) -> (Style, Style, Color) {
    let cc = change_color(rv);
    let recently = rv.recently_changed();
    let base = if is_selected {
        Style::default()
            .bg(Color::DarkGray)
            .fg(Color::White)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };
    let value = if recently && !is_selected {
        Style::default().fg(cc).add_modifier(Modifier::BOLD)
    } else {
        base
    };
    (base, value, cc)
}

fn format_addr(addr: u16, fmt: crate::app::AddrFormat) -> String {
    match fmt {
        crate::app::AddrFormat::Hex => format!("0x{:04X}", addr),
        crate::app::AddrFormat::Decimal => format!("{}", addr),
    }
}

/// Row for word registers: Addr, [Hex], Value (formatted), Timestamp, Label
#[allow(clippy::too_many_arguments)]
fn build_word_row<'a>(
    addr: u16,
    hex_str: &str,
    value_str: &str,
    rv: &RegisterValue,
    label: Option<&str>,
    is_selected: bool,
    addr_format: crate::app::AddrFormat,
    hide_hex: bool,
) -> Row<'a> {
    let (base, value_style, cc) = styles_for_row(rv, is_selected);

    let addr_str = format_addr(addr, addr_format);
    let addr_cell = if rv.recently_changed() && !is_selected {
        Cell::from(addr_str).style(Style::default().fg(cc))
    } else {
        Cell::from(addr_str).style(base)
    };

    let mut cells = vec![addr_cell];
    if !hide_hex {
        cells.push(Cell::from(hex_str.to_string()).style(value_style));
    }
    cells.push(Cell::from(value_str.to_string()).style(value_style));
    cells.push(Cell::from(rv.changed_wall.clone()).style(value_style));
    cells.push(Cell::from(label.unwrap_or("").to_string()).style(base));

    Row::new(cells)
}

/// Row for coils / discrete inputs: Addr, Value (ON/OFF), Timestamp, Label
fn build_coil_row<'a>(
    addr: u16,
    rv: &RegisterValue,
    is_selected: bool,
    addr_format: crate::app::AddrFormat,
) -> Row<'a> {
    let (base, _value_style, cc) = styles_for_row(rv, is_selected);
    let recently = rv.recently_changed();

    let addr_str = format_addr(addr, addr_format);
    let addr_cell = if recently && !is_selected {
        Cell::from(addr_str).style(Style::default().fg(cc))
    } else {
        Cell::from(addr_str).style(base)
    };

    let val_text = if rv.raw != 0 { "ON" } else { "OFF" };

    let val_style = if is_selected {
        base
    } else if recently {
        Style::default().fg(cc).add_modifier(Modifier::BOLD)
    } else {
        base
    };

    Row::new(vec![
        addr_cell,
        Cell::from(val_text).style(val_style),
        Cell::from(rv.changed_wall.clone()).style(val_style),
        Cell::from(rv.label.as_deref().unwrap_or("").to_string()).style(base),
    ])
}

// ---------------------------------------------------------------------------
// Log panel
// ---------------------------------------------------------------------------

fn draw_log(frame: &mut Frame, state: &AppState, area: Rect) {
    let focused = state.ui.focus == FocusPane::Log;
    let border_style = if focused {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let scroll_indicator = if state.ui.log_scroll > 0 {
        format!(" Log [+{}] ", state.ui.log_scroll)
    } else {
        " Log ".to_string()
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(scroll_indicator)
        .title_alignment(Alignment::Left);

    let entries = &state.log.entries;
    let inner_height = area.height.saturating_sub(2) as usize;
    let max_scroll = entries.len().saturating_sub(inner_height);
    let scroll = state.ui.log_scroll.min(max_scroll);
    let end = entries.len().saturating_sub(scroll);
    let start = end.saturating_sub(inner_height);

    let items: Vec<ListItem> = entries
        .iter()
        .skip(start)
        .take(end - start)
        .map(|entry| {
            let (level_style, level_tag) = match entry.level {
                LogLevel::Info => (Style::default().fg(Color::Green), "INFO"),
                LogLevel::Warn => (Style::default().fg(Color::Yellow), "WARN"),
                LogLevel::Error => (
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                    "ERR ",
                ),
            };
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("{} ", entry.wall_clock),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(format!("[{level_tag}] "), level_style),
                Span::raw(&entry.message),
            ]))
        })
        .collect();

    frame.render_widget(List::new(items).block(block), area);
}

// ---------------------------------------------------------------------------
// Bottom bar
// ---------------------------------------------------------------------------

fn draw_bottom_bar(frame: &mut Frame, state: &AppState, area: Rect) {
    if let InputMode::CommandBar { input, error } = &state.ui.input_mode {
        let text = if let Some(err) = error {
            Line::from(vec![
                Span::styled(":", Style::default().fg(Color::Yellow)),
                Span::raw(input.as_str()),
                Span::raw("  "),
                Span::styled(err.as_str(), Style::default().fg(Color::Red)),
            ])
        } else {
            Line::from(vec![
                Span::styled(":", Style::default().fg(Color::Yellow)),
                Span::raw(input.as_str()),
                Span::styled("_", Style::default().add_modifier(Modifier::SLOW_BLINK)),
            ])
        };
        frame.render_widget(
            Paragraph::new(text).style(Style::default().bg(Color::Rgb(30, 30, 30))),
            area,
        );
    }
}

// ---------------------------------------------------------------------------
// Write dialog overlay
// ---------------------------------------------------------------------------

fn draw_write_dialog(frame: &mut Frame, state: &AppState) {
    let (addr, tab_index, input, error) = match &state.ui.input_mode {
        InputMode::WriteDialog {
            addr,
            tab_index,
            input,
            error,
        } => (*addr, *tab_index, input.as_str(), error.as_deref()),
        _ => return,
    };

    let area = frame.area();
    let width = 55u16.min(area.width.saturating_sub(4));
    let height = 8u16.min(area.height.saturating_sub(2));
    let x = (area.width.saturating_sub(width)) / 2;
    let y = (area.height.saturating_sub(height)) / 2;
    let dialog_area = Rect::new(x, y, width, height);

    frame.render_widget(Clear, dialog_area);

    let pane_fmt = state
        .ui
        .panes
        .get(tab_index)
        .map(|p| p.addr_format)
        .unwrap_or_default();
    let range_label = state
        .config
        .ranges
        .get(tab_index)
        .map(|r| r.tab_label(state.config.start_reference, pane_fmt))
        .unwrap_or_default();

    let is_coils = state
        .config
        .ranges
        .get(tab_index)
        .map(|r| r.reg_type.is_coil_type())
        .unwrap_or(false);
    let nf = if is_coils {
        NumFormat::Uint16
    } else {
        state
            .ui
            .panes
            .get(tab_index)
            .map(|p| p.num_format)
            .unwrap_or_default()
    };

    // Show current value in the active numeric format
    let regs = state.registers.get(tab_index);
    let current_display = if let Some(map) = regs {
        let width = nf.width();
        // Collect consecutive registers starting at `addr`
        let vals: Vec<u16> = (0..width as u16)
            .map(|i| map.get(&(addr + i)).map(|rv| rv.raw).unwrap_or(0))
            .collect();
        let ws = crate::format::WordSwap {
            ints: state.config.swap_ints,
            floats: state.config.swap_floats,
            bytes: state.config.swap_bytes,
        };
        nf.format_value(&vals, &ws)
    } else {
        "?".to_string()
    };

    let display_addr = addr + state.config.start_reference;
    let title = format!(" Write {range_label} 0x{display_addr:04X} ");
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow))
        .title(Span::styled(
            title,
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ));

    let format_label = if is_coils {
        "0/1".to_string()
    } else {
        format!(
            "{} ({} reg{})",
            nf.column_header(),
            nf.width(),
            if nf.width() > 1 { "s" } else { "" }
        )
    };

    let mut lines = vec![
        Line::from(vec![
            Span::styled("  Format:  ", Style::default().fg(Color::DarkGray)),
            Span::styled(&format_label, Style::default().fg(Color::Cyan)),
        ]),
        Line::from(vec![
            Span::styled("  Current: ", Style::default().fg(Color::DarkGray)),
            Span::styled(&current_display, Style::default().fg(Color::White)),
        ]),
        Line::from(vec![
            Span::styled("  New value: ", Style::default().fg(Color::Cyan)),
            Span::raw(input),
            Span::styled("_", Style::default().add_modifier(Modifier::SLOW_BLINK)),
        ]),
    ];

    if let Some(err) = error {
        lines.push(Line::from(Span::styled(
            format!("  {err}"),
            Style::default().fg(Color::Red),
        )));
    } else {
        lines.push(Line::from(Span::styled(
            "  Enter=confirm  Esc=cancel  (dec, 0x hex, 0b bin)",
            Style::default().fg(Color::DarkGray),
        )));
    }

    frame.render_widget(Paragraph::new(lines).block(block), dialog_area);
}

// ---------------------------------------------------------------------------
// Label dialog overlay
// ---------------------------------------------------------------------------

fn draw_label_dialog(frame: &mut Frame, state: &AppState) {
    let (addr, tab_index, input) = match &state.ui.input_mode {
        InputMode::LabelDialog {
            addr,
            tab_index,
            input,
        } => (*addr, *tab_index, input.as_str()),
        _ => return,
    };

    let area = frame.area();
    let width = 50u16.min(area.width.saturating_sub(4));
    let height = 6u16.min(area.height.saturating_sub(2));
    let x = (area.width.saturating_sub(width)) / 2;
    let y = (area.height.saturating_sub(height)) / 2;
    let dialog_area = Rect::new(x, y, width, height);

    frame.render_widget(Clear, dialog_area);

    let pane_fmt = state
        .ui
        .panes
        .get(tab_index)
        .map(|p| p.addr_format)
        .unwrap_or_default();
    let display_addr = addr + state.config.start_reference;
    let addr_str = match pane_fmt {
        crate::app::AddrFormat::Hex => format!("0x{:04X}", display_addr),
        crate::app::AddrFormat::Decimal => format!("{}", display_addr),
    };
    let title = format!(" Label @{addr_str} ");
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(Span::styled(
            title,
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ));

    let lines = vec![
        Line::from(vec![
            Span::styled("  Label: ", Style::default().fg(Color::DarkGray)),
            Span::raw(input),
            Span::styled("_", Style::default().add_modifier(Modifier::SLOW_BLINK)),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "  Enter=confirm  Esc=cancel  (empty to clear)",
            Style::default().fg(Color::DarkGray),
        )),
    ];

    frame.render_widget(Paragraph::new(lines).block(block), dialog_area);
}

// ---------------------------------------------------------------------------
// Help dialog overlay
// ---------------------------------------------------------------------------

fn draw_help_dialog(frame: &mut Frame) {
    let area = frame.area();
    let width = 60u16.min(area.width.saturating_sub(4));
    let height = 26u16.min(area.height.saturating_sub(2));
    let x = (area.width.saturating_sub(width)) / 2;
    let y = (area.height.saturating_sub(height)) / 2;
    let dialog_area = Rect::new(x, y, width, height);

    frame.render_widget(Clear, dialog_area);

    let key_style = Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD);
    let desc_style = Style::default().fg(Color::White);
    let section_style = Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD);

    let help = |key: &str, desc: &str| -> Line<'static> {
        Line::from(vec![
            Span::styled(format!("  {:<18}", key), key_style),
            Span::styled(desc.to_string(), desc_style),
        ])
    };

    let lines = vec![
        Line::from(""),
        Line::from(Span::styled("  Navigation", section_style)),
        help("Tab / Shift+Tab", "Switch between register panes"),
        help("Up / k", "Move selection up"),
        help("Down / j", "Move selection down"),
        help("PageUp / PageDown", "Scroll by 20 rows"),
        help("Home / End", "Jump to first / last row"),
        help("F2", "Toggle focus: registers / log"),
        Line::from(""),
        Line::from(Span::styled("  Actions", section_style)),
        help("w", "Write value to selected register"),
        help("l", "Edit label for selected register"),
        help("f", "Choose numeric format for active pane"),
        help("d / D", "Decimal addresses (active / all panes)"),
        help("h / H", "Hex addresses (active / all panes)"),
        help(":", "Open command bar"),
        help("F1", "Show this help"),
        help("q / Esc", "Quit"),
        help("Ctrl+C", "Quit"),
        Line::from(""),
        Line::from(Span::styled("  Commands (via :)", section_style)),
        help(":poll <ms>", "Change poll interval"),
        help(":export [path]", "Export registers to JSON"),
        help(":save [path]", "Save current config for -c option"),
        Line::from(""),
        Line::from(Span::styled(
            "  Press any key to close",
            Style::default().fg(Color::DarkGray),
        )),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(Span::styled(
            " Help ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ));

    frame.render_widget(Paragraph::new(lines).block(block), dialog_area);
}

// ---------------------------------------------------------------------------
// Format dialog overlay
// ---------------------------------------------------------------------------

fn draw_format_dialog(frame: &mut Frame, selected: usize) {
    let area = frame.area();
    let item_count = NumFormat::ALL.len();
    let height = (item_count as u16 + 5).min(area.height.saturating_sub(2));
    let width = 50u16.min(area.width.saturating_sub(4));
    let x = (area.width.saturating_sub(width)) / 2;
    let y = (area.height.saturating_sub(height)) / 2;
    let dialog_area = Rect::new(x, y, width, height);

    frame.render_widget(Clear, dialog_area);

    let mut lines = vec![Line::from("")];

    for (i, fmt) in NumFormat::ALL.iter().enumerate() {
        let marker = if i == selected { ">" } else { " " };
        let style = if i == selected {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        lines.push(Line::from(Span::styled(
            format!("  {} {}", marker, fmt),
            style,
        )));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  Up/Down=navigate  Enter=select  Esc=cancel",
        Style::default().fg(Color::DarkGray),
    )));

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow))
        .title(Span::styled(
            " Numeric Format ",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ));

    frame.render_widget(Paragraph::new(lines).block(block), dialog_area);
}
