use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpStream;

use crate::app::{LogLevel, SharedState};

#[derive(Debug, Clone, Copy)]
enum PacketDirection {
    Tx,
    Rx,
}

/// A wrapper around `TcpStream` that captures all bytes flowing through
/// and logs them directly into `AppState` via `try_lock()`.
/// If the lock is contended, packets are buffered and flushed on the next
/// successful lock acquisition.
#[derive(Debug)]
pub struct LoggingTransport {
    inner: TcpStream,
    state: SharedState,
    pending: Vec<(PacketDirection, Vec<u8>)>,
}

impl LoggingTransport {
    pub fn new(inner: TcpStream, state: SharedState) -> Self {
        Self {
            inner,
            state,
            pending: Vec::new(),
        }
    }

    fn log_packet(&mut self, direction: PacketDirection, data: Vec<u8>) {
        self.pending.push((direction, data));
        self.flush_pending();
    }

    fn flush_pending(&mut self) {
        if self.pending.is_empty() {
            return;
        }
        if let Ok(mut s) = self.state.try_lock() {
            for (dir, data) in self.pending.drain(..) {
                let hex: Vec<String> = data.iter().map(|b| format!("{b:02X}")).collect();
                let level = match dir {
                    PacketDirection::Tx => LogLevel::PacketTx,
                    PacketDirection::Rx => LogLevel::PacketRx,
                };
                s.log.push(level, hex.join(" "));
            }
        }
    }
}

impl AsyncRead for LoggingTransport {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        let before = buf.filled().len();
        let result = Pin::new(&mut this.inner).poll_read(cx, buf);
        if let Poll::Ready(Ok(())) = &result {
            let after = buf.filled().len();
            if after > before {
                let data = buf.filled()[before..after].to_vec();
                this.log_packet(PacketDirection::Rx, data);
            }
        }
        result
    }
}

impl AsyncWrite for LoggingTransport {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        let result = Pin::new(&mut this.inner).poll_write(cx, buf);
        if let Poll::Ready(Ok(n)) = &result
            && *n > 0
        {
            let data = buf[..*n].to_vec();
            this.log_packet(PacketDirection::Tx, data);
        }
        result
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_shutdown(cx)
    }
}
