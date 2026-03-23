use std::collections::HashMap;
use std::future;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex as StdMutex};

use tokio::net::TcpListener;
use tokio_modbus::server::Service;
use tokio_modbus::server::tcp::Server;
use tokio_modbus::{ExceptionCode, Request, Response};

use crate::app::{SharedState, ShutdownRx};

// ---------------------------------------------------------------------------
// Register data store (shared across connections via Arc<StdMutex<..>>)
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Clone)]
pub struct RegisterStore {
    pub coils: HashMap<u16, bool>,
    pub discrete_inputs: HashMap<u16, bool>,
    pub holding_registers: HashMap<u16, u16>,
    pub input_registers: HashMap<u16, u16>,
}

impl RegisterStore {
    /// Populate from the config's `initial_values` map.
    /// Keys are "type:address", e.g. "hr:0", "co:5", "ir:100", "di:0".
    pub fn from_initial_values(values: &HashMap<String, u16>) -> Self {
        let mut store = Self::default();
        for (key, &val) in values {
            let parts: Vec<&str> = key.split(':').collect();
            if parts.len() != 2 {
                continue;
            }
            let Ok(addr) = parts[1].parse::<u16>() else {
                continue;
            };
            match parts[0] {
                "hr" => {
                    store.holding_registers.insert(addr, val);
                }
                "ir" => {
                    store.input_registers.insert(addr, val);
                }
                "co" => {
                    store.coils.insert(addr, val != 0);
                }
                "di" => {
                    store.discrete_inputs.insert(addr, val != 0);
                }
                _ => {}
            }
        }
        store
    }

    fn read_coils(&self, addr: u16, count: u16) -> Vec<bool> {
        (0..count)
            .map(|i| *self.coils.get(&(addr + i)).unwrap_or(&false))
            .collect()
    }

    fn read_discrete_inputs(&self, addr: u16, count: u16) -> Vec<bool> {
        (0..count)
            .map(|i| *self.discrete_inputs.get(&(addr + i)).unwrap_or(&false))
            .collect()
    }

    fn read_holding_registers(&self, addr: u16, count: u16) -> Vec<u16> {
        (0..count)
            .map(|i| *self.holding_registers.get(&(addr + i)).unwrap_or(&0))
            .collect()
    }

    fn read_input_registers(&self, addr: u16, count: u16) -> Vec<u16> {
        (0..count)
            .map(|i| *self.input_registers.get(&(addr + i)).unwrap_or(&0))
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Log message sent from service → background drainer → AppState
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum ServerEvent {
    Log(String),
    ClientConnected(SocketAddr),
    ClientDisconnected(SocketAddr),
    RequestCoils,
    RequestDiscreteInputs,
    RequestHoldingRegisters,
    RequestInputRegisters,
    RequestWrite,
    RequestOther,
}

// ---------------------------------------------------------------------------
// Modbus service implementation
// ---------------------------------------------------------------------------

pub struct ModbusService {
    store: Arc<StdMutex<RegisterStore>>,
    event_tx: tokio::sync::mpsc::UnboundedSender<ServerEvent>,
    peer: SocketAddr,
}

impl Service for ModbusService {
    type Request = Request<'static>;
    type Response = Response;
    type Exception = ExceptionCode;
    type Future = future::Ready<Result<Self::Response, Self::Exception>>;

    fn call(&self, req: Self::Request) -> Self::Future {
        let result = self.handle_request(req);
        future::ready(result)
    }
}

impl ModbusService {
    fn handle_request(&self, req: Request<'static>) -> Result<Response, ExceptionCode> {
        let mut store = self.store.lock().unwrap();

        match req {
            Request::ReadCoils(addr, count) => {
                let _ = self.event_tx.send(ServerEvent::RequestCoils);
                let _ = self.event_tx.send(ServerEvent::Log(format!(
                    "[{}] ReadCoils addr=0x{:04X} count={}",
                    self.peer, addr, count
                )));
                let values = store.read_coils(addr, count);
                Ok(Response::ReadCoils(values))
            }

            Request::ReadDiscreteInputs(addr, count) => {
                let _ = self.event_tx.send(ServerEvent::RequestDiscreteInputs);
                let _ = self.event_tx.send(ServerEvent::Log(format!(
                    "[{}] ReadDiscreteInputs addr=0x{:04X} count={}",
                    self.peer, addr, count
                )));
                let values = store.read_discrete_inputs(addr, count);
                Ok(Response::ReadDiscreteInputs(values))
            }

            Request::ReadHoldingRegisters(addr, count) => {
                let _ = self.event_tx.send(ServerEvent::RequestHoldingRegisters);
                let _ = self.event_tx.send(ServerEvent::Log(format!(
                    "[{}] ReadHoldingRegisters addr=0x{:04X} count={}",
                    self.peer, addr, count
                )));
                let values = store.read_holding_registers(addr, count);
                Ok(Response::ReadHoldingRegisters(values))
            }

            Request::ReadInputRegisters(addr, count) => {
                let _ = self.event_tx.send(ServerEvent::RequestInputRegisters);
                let _ = self.event_tx.send(ServerEvent::Log(format!(
                    "[{}] ReadInputRegisters addr=0x{:04X} count={}",
                    self.peer, addr, count
                )));
                let values = store.read_input_registers(addr, count);
                Ok(Response::ReadInputRegisters(values))
            }

            Request::WriteSingleCoil(addr, value) => {
                let _ = self.event_tx.send(ServerEvent::RequestWrite);
                let _ = self.event_tx.send(ServerEvent::Log(format!(
                    "[{}] WriteSingleCoil addr=0x{:04X} value={}",
                    self.peer, addr, value
                )));
                store.coils.insert(addr, value);
                Ok(Response::WriteSingleCoil(addr, value))
            }

            Request::WriteSingleRegister(addr, value) => {
                let _ = self.event_tx.send(ServerEvent::RequestWrite);
                let _ = self.event_tx.send(ServerEvent::Log(format!(
                    "[{}] WriteSingleRegister addr=0x{:04X} value={}",
                    self.peer, addr, value
                )));
                store.holding_registers.insert(addr, value);
                Ok(Response::WriteSingleRegister(addr, value))
            }

            Request::WriteMultipleCoils(addr, values) => {
                let _ = self.event_tx.send(ServerEvent::RequestWrite);
                let count = values.len() as u16;
                let _ = self.event_tx.send(ServerEvent::Log(format!(
                    "[{}] WriteMultipleCoils addr=0x{:04X} count={}",
                    self.peer, addr, count
                )));
                for (i, &val) in values.iter().enumerate() {
                    store.coils.insert(addr + i as u16, val);
                }
                Ok(Response::WriteMultipleCoils(addr, count))
            }

            Request::WriteMultipleRegisters(addr, values) => {
                let _ = self.event_tx.send(ServerEvent::RequestWrite);
                let count = values.len() as u16;
                let _ = self.event_tx.send(ServerEvent::Log(format!(
                    "[{}] WriteMultipleRegisters addr=0x{:04X} count={}",
                    self.peer, addr, count
                )));
                for (i, &val) in values.iter().enumerate() {
                    store.holding_registers.insert(addr + i as u16, val);
                }
                Ok(Response::WriteMultipleRegisters(addr, count))
            }

            _ => {
                let _ = self.event_tx.send(ServerEvent::RequestOther);
                let _ = self.event_tx.send(ServerEvent::Log(format!(
                    "[{}] unsupported function code {:?}",
                    self.peer,
                    req.function_code()
                )));
                Err(ExceptionCode::IllegalFunction)
            }
        }
    }
}

// Guard that sends a disconnect event when dropped (connection closed)
struct DisconnectGuard {
    peer: SocketAddr,
    event_tx: tokio::sync::mpsc::UnboundedSender<ServerEvent>,
}

impl Drop for DisconnectGuard {
    fn drop(&mut self) {
        let _ = self
            .event_tx
            .send(ServerEvent::ClientDisconnected(self.peer));
    }
}

// ---------------------------------------------------------------------------
// Spawn the server
// ---------------------------------------------------------------------------

pub fn spawn(state: SharedState, shutdown_rx: ShutdownRx) {
    tokio::spawn(async move { run(state, shutdown_rx).await });
}

async fn run(state: SharedState, mut shutdown_rx: ShutdownRx) {
    let (host, port, initial_values) = {
        let s = state.lock().await;
        (
            s.config.host.clone(),
            s.config.port,
            s.config.initial_values.clone(),
        )
    };

    let bind_addr = format!("{host}:{port}");
    let socket_addr: SocketAddr = match bind_addr.parse() {
        Ok(a) => a,
        Err(e) => {
            let mut s = state.lock().await;
            let msg = format!("invalid bind address \"{bind_addr}\": {e}");
            s.log.error(&msg);
            s.connection = crate::app::ConnectionStatus::Error(msg);
            return;
        }
    };

    let listener = match TcpListener::bind(socket_addr).await {
        Ok(l) => {
            let mut s = state.lock().await;
            s.connection = crate::app::ConnectionStatus::Connected;
            s.log.info(format!("server listening on {socket_addr}"));
            l
        }
        Err(e) => {
            let mut s = state.lock().await;
            let msg = format!("bind failed: {e}");
            s.log.error(&msg);
            s.connection = crate::app::ConnectionStatus::Error(msg);
            return;
        }
    };

    let store = Arc::new(StdMutex::new(RegisterStore::from_initial_values(
        &initial_values,
    )));

    let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel();

    // Spawn the event drainer that pushes ServerEvents into AppState
    tokio::spawn(drain_events(event_rx, state.clone()));

    let server = Server::new(listener);

    let on_connected = |stream, socket_addr: SocketAddr| {
        let store = Arc::clone(&store);
        let event_tx = event_tx.clone();
        async move {
            let _ = event_tx.send(ServerEvent::ClientConnected(socket_addr));
            let service = ModbusService {
                store,
                event_tx: event_tx.clone(),
                peer: socket_addr,
            };
            // Wrap service in an Arc so it's Clone + Send + Sync.
            // Spawn a disconnect guard as a tokio task-local won't work,
            // but we can use a wrapper.
            let guard = Arc::new(DisconnectGuard {
                peer: socket_addr,
                event_tx,
            });
            let svc = GuardedService {
                service,
                _guard: guard,
            };
            Ok(Some((svc, stream)))
        }
    };

    let on_process_error = |err: std::io::Error| {
        // Per-connection error — nothing to do, already logged via disconnect
        let _ = err;
    };

    let abort_signal = async move {
        // Fires when the sender is dropped or sends `true`
        let _ = shutdown_rx.changed().await;
    };

    let _ = server
        .serve_until(&on_connected, on_process_error, abort_signal)
        .await;
}

/// Wrapper that holds the disconnect guard alongside the service.
struct GuardedService {
    service: ModbusService,
    _guard: Arc<DisconnectGuard>,
}

impl Service for GuardedService {
    type Request = Request<'static>;
    type Response = Response;
    type Exception = ExceptionCode;
    type Future = future::Ready<Result<Self::Response, Self::Exception>>;

    fn call(&self, req: Self::Request) -> Self::Future {
        self.service.call(req)
    }
}

// ---------------------------------------------------------------------------
// Event drainer — reads from channel, updates AppState
// ---------------------------------------------------------------------------

async fn drain_events(
    mut rx: tokio::sync::mpsc::UnboundedReceiver<ServerEvent>,
    state: SharedState,
) {
    while let Some(event) = rx.recv().await {
        let mut s = state.lock().await;
        match event {
            ServerEvent::Log(msg) => {
                s.log.info(msg);
            }
            ServerEvent::ClientConnected(addr) => {
                s.server.active_connections += 1;
                s.server.total_connections += 1;
                s.log.info(format!("client connected: {addr}"));
            }
            ServerEvent::ClientDisconnected(addr) => {
                s.server.active_connections = s.server.active_connections.saturating_sub(1);
                s.log.info(format!("client disconnected: {addr}"));
            }
            ServerEvent::RequestCoils => {
                s.server.requests_coils += 1;
            }
            ServerEvent::RequestDiscreteInputs => {
                s.server.requests_discrete_inputs += 1;
            }
            ServerEvent::RequestHoldingRegisters => {
                s.server.requests_holding_registers += 1;
            }
            ServerEvent::RequestInputRegisters => {
                s.server.requests_input_registers += 1;
            }
            ServerEvent::RequestWrite => {
                s.server.requests_write += 1;
            }
            ServerEvent::RequestOther => {
                s.server.requests_other += 1;
            }
        }
    }
}
