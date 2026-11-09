//! Length-prefixed message framing over non-blocking TCP (std only).
//!
//! Every frame is a little-endian `u32` byte count followed by the payload. Sockets are
//! non-blocking: `pump` moves whatever bytes the OS has ready in both directions and never
//! waits, so it is safe to call once per game frame.
use std::io::{ErrorKind, Read, Write};
use std::net::TcpStream;

/// Largest accepted payload. Anything bigger is treated as a broken / hostile peer.
pub const MAX_FRAME: usize = 1 << 20;
/// A peer that stops reading is dropped once this much output is queued for it.
pub const MAX_BACKLOG: usize = 8 << 20;

pub struct Conn {
    stream: TcpStream,
    rx: Vec<u8>,
    rx_pos: usize,
    tx: Vec<u8>,
    /// set when the socket closed or errored; the owner should drop the connection
    pub dead: bool,
    /// seconds since anything was received (maintained by the owner via `tick`)
    pub idle: f32,
}

impl Conn {
    pub fn new(stream: TcpStream) -> std::io::Result<Conn> {
        stream.set_nonblocking(true)?;
        stream.set_nodelay(true)?;
        Ok(Conn { stream, rx: Vec::with_capacity(4096), rx_pos: 0, tx: Vec::with_capacity(4096), dead: false, idle: 0.0 })
    }

    pub fn peer(&self) -> String {
        self.stream.peer_addr().map(|a| a.to_string()).unwrap_or_else(|_| "?".into())
    }

    /// Queue one frame for sending.
    pub fn send_frame(&mut self, payload: &[u8]) {
        if self.dead {
            return;
        }
        if payload.len() > MAX_FRAME {
            self.dead = true;
            return;
        }
        self.tx.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        self.tx.extend_from_slice(payload);
        if self.tx.len() > MAX_BACKLOG {
            self.dead = true;
        }
    }

    /// Write queued bytes and read available bytes without blocking.
    pub fn pump(&mut self) {
        if self.dead {
            return;
        }
        // ---- write
        let mut written = 0;
        while written < self.tx.len() {
            match self.stream.write(&self.tx[written..]) {
                Ok(0) => {
                    self.dead = true;
                    break;
                }
                Ok(n) => written += n,
                Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                Err(_) => {
                    self.dead = true;
                    break;
                }
            }
        }
        if written > 0 {
            self.tx.drain(..written);
        }
        // ---- read
        let mut buf = [0u8; 16 * 1024];
        loop {
            match self.stream.read(&mut buf) {
                Ok(0) => {
                    self.dead = true;
                    break;
                }
                Ok(n) => {
                    self.rx.extend_from_slice(&buf[..n]);
                    self.idle = 0.0;
                    if self.rx.len() - self.rx_pos > MAX_BACKLOG {
                        self.dead = true;
                        break;
                    }
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                Err(_) => {
                    self.dead = true;
                    break;
                }
            }
        }
    }

    /// Next complete frame that has been received, if any.
    pub fn next_frame(&mut self) -> Option<Vec<u8>> {
        let avail = self.rx.len() - self.rx_pos;
        if avail < 4 {
            self.compact();
            return None;
        }
        let p = self.rx_pos;
        let len = u32::from_le_bytes([self.rx[p], self.rx[p + 1], self.rx[p + 2], self.rx[p + 3]]) as usize;
        if len > MAX_FRAME {
            self.dead = true;
            return None;
        }
        if avail < 4 + len {
            self.compact();
            return None;
        }
        let out = self.rx[p + 4..p + 4 + len].to_vec();
        self.rx_pos += 4 + len;
        Some(out)
    }

    fn compact(&mut self) {
        if self.rx_pos > 0 {
            self.rx.drain(..self.rx_pos);
            self.rx_pos = 0;
        }
    }

    /// Best effort: push out what is queued, then close.
    pub fn close(&mut self) {
        let _ = self.stream.set_nonblocking(false);
        let _ = self.stream.set_write_timeout(Some(std::time::Duration::from_millis(200)));
        let _ = self.stream.write_all(&self.tx);
        let _ = self.stream.shutdown(std::net::Shutdown::Both);
        self.tx.clear();
        self.dead = true;
    }

    pub fn tick(&mut self, dt: f32) {
        self.idle += dt;
    }
}
