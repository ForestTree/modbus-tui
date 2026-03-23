use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

use tokio_modbus::Slave;
use tokio_modbus::client::{Context, Reader, Writer, tcp};

use crate::app::{ConnectionStatus, RegisterValue, SharedState, WriteRx};
use crate::config::{PollRange, RegisterType};

const INITIAL_BACKOFF: Duration = Duration::from_secs(1);
const MAX_BACKOFF: Duration = Duration::from_secs(10);

pub fn spawn(state: SharedState, write_rx: WriteRx) {
    tokio::spawn(run(state, write_rx));
}

async fn run(state: SharedState, mut write_rx: WriteRx) {
    let mut backoff = INITIAL_BACKOFF;

    loop {
        let (running, addr, slave, ranges, sr) = {
            let s = state.lock().await;
            (
                s.running,
                format!("{}:{}", s.config.host, s.config.port),
                Slave(s.config.unit),
                s.config.ranges.clone(),
                s.config.start_reference,
            )
        };
        if !running {
            return;
        }

        let socket_addr: SocketAddr = match addr.parse() {
            Ok(a) => a,
            Err(e) => {
                let mut s = state.lock().await;
                let msg = format!("invalid address \"{addr}\": {e}");
                s.log.error(&msg);
                s.connection = ConnectionStatus::Error(msg);
                return;
            }
        };

        // --- connect ---
        {
            let mut s = state.lock().await;
            s.connection = ConnectionStatus::Connecting;
            s.log
                .info(format!("connecting to {socket_addr} slave={}", slave.0));
        }

        let mut ctx = match tcp::connect_slave(socket_addr, slave).await {
            Ok(ctx) => {
                let mut s = state.lock().await;
                s.connection = ConnectionStatus::Connected;
                s.log.info("connected");
                backoff = INITIAL_BACKOFF;
                ctx
            }
            Err(e) => {
                let mut s = state.lock().await;
                let msg = format!("connect failed: {e}");
                s.log.error(&msg);
                s.connection = ConnectionStatus::Error(msg);
                drop(s);
                if !wait_or_stop(&state, backoff).await {
                    return;
                }
                backoff = (backoff + Duration::from_secs(1)).min(MAX_BACKOFF);
                continue;
            }
        };

        // --- poll loop ---
        loop {
            let cycle_start = Instant::now();

            // Single lock to read poll interval and check running
            let poll_interval = {
                let s = state.lock().await;
                if !s.running {
                    return;
                }
                Duration::from_millis(s.config.poll_interval_ms)
            };

            // Drain pending write requests
            while let Ok(req) = write_rx.try_recv() {
                let reg_type = ranges.get(req.tab_index).map(|r| r.reg_type);
                if let Some(rt) = reg_type
                    && let Err(msg) =
                        execute_write(&mut ctx, rt, req.addr, &req.values, &state).await
                {
                    let mut s = state.lock().await;
                    s.log.error(format!("write failed: {msg}"));
                }
            }

            let mut had_error = false;

            // Read each configured range
            for (i, range) in ranges.iter().enumerate() {
                match read_range(&mut ctx, range, i, &state).await {
                    Ok(()) => {}
                    Err(msg) => {
                        let mut s = state.lock().await;
                        s.log.error(format!(
                            "{}: {msg}",
                            range.tab_label(sr, crate::app::AddrFormat::default())
                        ));
                        had_error = true;
                        break;
                    }
                }
            }

            if had_error {
                let mut s = state.lock().await;
                let msg = "read error — will reconnect".to_string();
                s.connection = ConnectionStatus::Error(msg);
                drop(s);
                wait_or_stop(&state, backoff).await;
                backoff = (backoff * 2).min(MAX_BACKOFF);
                break;
            }

            // Sleep only for the remaining time (poll_interval minus work done)
            let remaining = poll_interval.saturating_sub(cycle_start.elapsed());
            if !wait_or_stop(&state, remaining).await {
                return;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Read a single range and store into the corresponding registers[index]
// ---------------------------------------------------------------------------

async fn read_range(
    ctx: &mut Context,
    range: &PollRange,
    index: usize,
    state: &SharedState,
) -> Result<(), String> {
    match range.reg_type {
        RegisterType::HoldingRegisters => {
            let data = ctx
                .read_holding_registers(range.start, range.count)
                .await
                .map_err(|e| format!("{e}"))?
                .map_err(|ex| format!("exception: {ex}"))?;
            let mut s = state.lock().await;
            store_word_values(&mut s.registers[index], range.start, &data);
        }
        RegisterType::InputRegisters => {
            let data = ctx
                .read_input_registers(range.start, range.count)
                .await
                .map_err(|e| format!("{e}"))?
                .map_err(|ex| format!("exception: {ex}"))?;
            let mut s = state.lock().await;
            store_word_values(&mut s.registers[index], range.start, &data);
        }
        RegisterType::Coils => {
            let data = ctx
                .read_coils(range.start, range.count)
                .await
                .map_err(|e| format!("{e}"))?
                .map_err(|ex| format!("exception: {ex}"))?;
            let mut s = state.lock().await;
            store_bool_values(&mut s.registers[index], range.start, &data);
        }
        RegisterType::DiscreteInputs => {
            let data = ctx
                .read_discrete_inputs(range.start, range.count)
                .await
                .map_err(|e| format!("{e}"))?
                .map_err(|ex| format!("exception: {ex}"))?;
            let mut s = state.lock().await;
            store_bool_values(&mut s.registers[index], range.start, &data);
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Write execution
// ---------------------------------------------------------------------------

async fn execute_write(
    ctx: &mut Context,
    reg_type: RegisterType,
    addr: u16,
    values: &[u16],
    state: &SharedState,
) -> Result<(), String> {
    if values.is_empty() {
        return Err("no values to write".to_string());
    }
    match reg_type {
        RegisterType::HoldingRegisters => {
            if values.len() == 1 {
                // Function code 6: Preset Single Register
                ctx.write_single_register(addr, values[0])
                    .await
                    .map_err(|e| format!("{e}"))?
                    .map_err(|ex| format!("exception: {ex}"))?;
                let mut s = state.lock().await;
                s.log.info(format!(
                    "wrote register 0x{:04X} = {} (0x{:04X})",
                    addr, values[0], values[0]
                ));
            } else {
                // Function code 16: Preset Multiple Registers
                ctx.write_multiple_registers(addr, values)
                    .await
                    .map_err(|e| format!("{e}"))?
                    .map_err(|ex| format!("exception: {ex}"))?;
                let vals_str: Vec<String> = values.iter().map(|v| format!("0x{:04X}", v)).collect();
                let mut s = state.lock().await;
                s.log.info(format!(
                    "wrote {} registers at 0x{:04X}: [{}]",
                    values.len(),
                    addr,
                    vals_str.join(", ")
                ));
            }
        }
        RegisterType::Coils => {
            ctx.write_single_coil(addr, values[0] != 0)
                .await
                .map_err(|e| format!("{e}"))?
                .map_err(|ex| format!("exception: {ex}"))?;
            let mut s = state.lock().await;
            s.log
                .info(format!("wrote coil 0x{:04X} = {}", addr, values[0] != 0));
        }
        _ => {
            return Err("cannot write to read-only register type".to_string());
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Store helpers
// ---------------------------------------------------------------------------

fn store_word_values(map: &mut BTreeMap<u16, RegisterValue>, start: u16, values: &[u16]) {
    for (i, &val) in values.iter().enumerate() {
        let addr = start + i as u16;
        match map.get_mut(&addr) {
            Some(rv) => rv.update(val),
            None => {
                map.insert(addr, RegisterValue::new(val));
            }
        }
    }
}

fn store_bool_values(map: &mut BTreeMap<u16, RegisterValue>, start: u16, values: &[bool]) {
    for (i, &val) in values.iter().enumerate() {
        let addr = start + i as u16;
        let raw = val as u16;
        match map.get_mut(&addr) {
            Some(rv) => rv.update(raw),
            None => {
                map.insert(addr, RegisterValue::new(raw));
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Utilities
// ---------------------------------------------------------------------------

/// Sleep for `duration`, checking the shutdown flag every 100 ms.
/// Returns `true` if still running, `false` if shutdown was requested.
async fn wait_or_stop(state: &SharedState, duration: Duration) -> bool {
    if duration.is_zero() {
        return state.lock().await.running;
    }
    let step = Duration::from_millis(100);
    let mut remaining = duration;
    while !remaining.is_zero() {
        let tick = remaining.min(step);
        tokio::time::sleep(tick).await;
        remaining = remaining.saturating_sub(tick);
        if !state.lock().await.running {
            return false;
        }
    }
    true
}
